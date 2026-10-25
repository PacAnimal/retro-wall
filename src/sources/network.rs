//! The status line's readings: online or not, this machine's address, its router's, the public one.

use crate::platform::{Native, Platform};
use crate::scene::Net;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Documentation addresses, for --demo and snapshots.
pub fn demo() -> Net {
    Net { online: Some(true), address: Some("192.168.1.37".into()), router: Some("192.168.1.1".into()), public: Some("203.0.113.37".into()) }
}

fn public_address(url: &str) -> Option<String> {
    let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().new_agent();
    let text = agent.get(url).call().ok()?.body_mut().read_to_string().ok()?;
    let text = text.trim().to_string();
    let ok = text.split('.').count() == 4 && text.split('.').all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()));
    ok.then_some(text)
}

/// On its own thread: local addresses every 5 s, the public one every 30 s (10 s while offline, and
/// at once when the addresses change).
pub fn spawn_network(state: Arc<Mutex<Net>>, url: String, running: Arc<AtomicBool>) {
    let spawned = std::thread::Builder::new().name("network".into()).spawn(move || {
        let mut asked: Option<Instant> = None;
        let mut local: Option<(Option<String>, Option<String>)> = None;
        loop {
            if running.load(Ordering::Relaxed) {
                let (address, router) = Native::addresses();
                let (mut online, mut public) = {
                    let s = state.lock().unwrap_or_else(|e| e.into_inner());
                    (s.online, s.public.clone())
                };
                if address.is_none() {
                    online = Some(false);
                    public = None;
                } else if local.as_ref() != Some(&(address.clone(), router.clone()))
                    || asked.is_none_or(|a| a.elapsed().as_secs_f64() >= if online == Some(true) { 30.0 } else { 10.0 })
                {
                    public = public_address(&url);
                    asked = Some(Instant::now());
                    online = Some(public.is_some());
                }
                local = Some((address.clone(), router.clone()));
                *state.lock().unwrap_or_else(|e| e.into_inner()) = Net { online, address, router, public };
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
    if let Err(e) = spawned {
        crate::errln!("network status: {e}");
    }
}
