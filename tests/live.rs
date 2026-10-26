//! The live wall, end to end, under Xvfb: it draws, it pauses (and then costs next to nothing) while a
//! VNC viewer is connected and comes back when it leaves, and hostile plugins neither stop nor stall it.

#![cfg(target_os = "linux")]

mod common;

use common::procs::{eventually, scratch};
use common::x::{Xvfb, hz, ticks};
use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// A running wall, its output lines arriving on a channel; killed when dropped.
struct Wall {
    child: Child,
    said: Receiver<String>,
    seen: Vec<String>,
    /// Lines before this one are not looked at again by `next`.
    from: usize,
}

impl Wall {
    fn start(x: &Xvfb, home: &Path, args: &[&str]) -> Wall {
        let mut c = x.retro_wall(home);
        // software GL animates only when asked; the public address from nowhere, so no network is used
        c.args(["--software", "--ip-url", "http://127.0.0.1:9/", "--theme", "phosphor", "--host", "testhost"]).args(args);
        let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("the binary runs");
        let (tx, said) = channel();
        for out in [Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>, Box::new(child.stderr.take().unwrap())] {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        Wall { child, said, seen: vec![], from: 0 }
    }

    /// Wait up to `secs` for a line containing `what`, said at any time.
    fn wait_for(&mut self, what: &str, secs: f64) -> bool {
        self.find(what, secs, 0).is_some()
    }

    /// Wait up to `secs` for a line containing `what`, said after the last one `next` found.
    fn next(&mut self, what: &str, secs: f64) -> bool {
        self.find(what, secs, self.from).map(|i| self.from = i + 1).is_some()
    }

    fn find(&mut self, what: &str, secs: f64, from: usize) -> Option<usize> {
        let end = std::time::Instant::now() + retro_wall::util::seconds(secs);
        loop {
            if let Some(i) = self.seen.iter().skip(from).position(|l| l.contains(what)) {
                return Some(from + i);
            }
            let line = self.said.recv_timeout(end.saturating_duration_since(std::time::Instant::now())).ok()?;
            self.seen.push(line);
        }
    }

    fn all_said(&mut self) -> String {
        while let Ok(l) = self.said.try_recv() {
            self.seen.push(l);
        }
        self.seen.join("\n")
    }

    fn alive(&mut self) -> bool {
        self.child.try_wait().is_ok_and(|s| s.is_none())
    }

    /// Its CPU use over `secs`, in percent of one core.
    fn cpu(&self, secs: f64) -> f64 {
        let pid = self.child.id();
        let before = ticks(pid);
        std::thread::sleep(retro_wall::util::seconds(secs));
        (ticks(pid) - before) as f64 / hz() as f64 / secs * 100.0
    }
}

impl Drop for Wall {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn it_pauses_for_a_vnc_viewer_and_resumes() {
    let Some(x) = Xvfb::start("it_pauses_for_a_vnc_viewer_and_resumes", 640, 360) else { return };
    let home = scratch("pause");
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port().to_string();
    let mut wall = Wall::start(&x, &home, &["--pause-vnc", &port]);
    assert!(wall.wait_for("textures, made in", 20.0), "the wall did not start:\n{}", wall.all_said());
    std::thread::sleep(Duration::from_secs(1));
    let drawing = wall.cpu(2.0);

    let viewer = TcpStream::connect(("127.0.0.1", server.local_addr().unwrap().port())).unwrap();
    let (accepted, _) = server.accept().unwrap();
    assert!(wall.next("paused: a VNC viewer is connected", 6.0), "not paused for the viewer:\n{}", wall.all_said());
    std::thread::sleep(Duration::from_secs(2)); // its fade out
    let paused = wall.cpu(3.0);

    drop(viewer);
    drop(accepted);
    assert!(wall.next("running", 8.0), "not running again after the viewer left:\n{}", wall.all_said());
    std::thread::sleep(Duration::from_secs(1));
    let again = wall.cpu(2.0);
    eprintln!("CPU, % of a core: drawing {drawing:.1}, paused {paused:.1}, resumed {again:.1}");
    assert!(wall.alive(), "it stopped:\n{}", wall.all_said());
    assert!(paused < 3.0, "paused, it still used {paused:.1}% of a core");
    assert!(drawing > 4.0 * paused.max(0.5), "drawing ({drawing:.1}%) is not clearly more than paused ({paused:.1}%)");
    assert!(again > 4.0 * paused.max(0.5), "after resuming ({again:.1}%) it is not drawing again (paused: {paused:.1}%)");
    let _ = std::fs::remove_dir_all(&home);
}

/// Sources that hang, print garbage, die or stream, and faces that answer huge numbers, endless output
/// or no JSON at all: each said, and the wall keeps drawing.
#[test]
fn hostile_plugins_neither_stop_nor_stall_the_wall() {
    let Some(x) = Xvfb::start("hostile_plugins_neither_stop_nor_stall_the_wall", 640, 360) else { return };
    let home = scratch("hostile");
    let conf = home.join(".config/retro");
    let script = |dir: &str, name: &str, body: &str| {
        let p = conf.join(dir).join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    script("sources", "hangs", "exec sleep 1000");
    script("sources", "garbage", "echo 'not a reading'");
    script("sources", "dies", "kill -KILL $$");
    script("sources", "stream", "echo '{\"stream\": true}'; i=0; while :; do i=$((i % 100 + 1)); echo $i; sleep 0.05; done");
    script(
        "faces",
        "huge",
        "while read -r _; do echo '[{\"op\": \"rect\", \"x\": -1e308, \"y\": 1e308, \"w\": 1e308, \"h\": 1e308}, {\"op\": \"fill\"}, {\"op\": \"text\", \"text\": \"x\", \"size\": 1e300}]'; done",
    );
    script("faces", "endless", "while read -r _; do yes hostile-endless | tr -d '\\n'; done");
    script("faces", "notjson", "while read -r _; do echo '{[}'; done");
    let widgets =
        [("a", "meter", "hangs"), ("b", "meter", "garbage"), ("c", "spark", "dies"), ("d", "huge", "stream"), ("e", "endless", "cpu"), ("f", "notjson", "ram")];
    let mut toml = format!("[[group]]\nanchor = \"top-left\"\nwidgets = [{}]\n", widgets.iter().map(|w| format!("\"{}\"", w.0)).collect::<Vec<_>>().join(", "));
    for (name, kind, source) in widgets {
        toml += &format!("[widget.{name}]\ntype = \"{kind}\"\nsource = \"{source}\"\nlabel = \"{name}\"\n");
    }
    std::fs::write(conf.join("retro.toml"), toml).unwrap();

    let mut wall = Wall::start(&x, &home, &[]);
    assert!(wall.wait_for("textures, made in", 20.0), "the wall did not start:\n{}", wall.all_said());
    for what in ["face", "source garbage", "source dies"] {
        assert!(wall.wait_for(what, 10.0), "nothing said of {what}:\n{}", wall.all_said());
    }
    assert!(wall.wait_for("source hangs", 10.0), "the hanging source was not said:\n{}", wall.all_said());
    for face in ["endless", "notjson"] {
        assert!(wall.wait_for(&format!("faces/{face}"), 5.0), "the face {face} was not said:\n{}", wall.all_said());
    }
    let cpu = wall.cpu(2.0);
    let said = wall.all_said();
    assert!(wall.alive(), "it stopped:\n{said}");
    assert!(!said.contains("faces/huge"), "huge numbers were refused, not drawn:\n{said}");
    assert!(cpu > 2.0, "it is not drawing ({cpu:.1}% of a core):\n{said}");
    // what the plugins started is gone with them: no endless `yes` left behind
    let leftover = || {
        std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter(|e| std::fs::read_to_string(e.path().join("cmdline")).is_ok_and(|c| c == "yes\0hostile-endless\0"))
            .count()
    };
    assert!(eventually(3.0, || leftover() == 0), "the endless face's pipeline was left running");
    let _ = std::fs::remove_dir_all(&home);
}

/// Text with a NUL in it, from a source (shown by a ticker), a face and the config (a widget's label):
/// cairo cannot take a NUL, and none stops the wall.
#[test]
fn text_with_a_nul_does_not_stop_the_wall() {
    let Some(x) = Xvfb::start("text_with_a_nul_does_not_stop_the_wall", 640, 360) else { return };
    let home = scratch("nul");
    let conf = home.join(".config/retro");
    let script = |dir: &str, name: &str, body: &str| {
        let p = conf.join(dir).join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    script("sources", "news", r#"echo $$ >"$HOME/news.pid"; printf '%s\n' '{"value": 5, "text": "news\u0000here"}'"#);
    script("faces", "nul", r#"while read -r _; do echo $$ >"$HOME/face.pid"; printf '%s\n' '[{"op": "text", "text": "\u0000x", "x": 4, "y": 20}]'; done"#);
    let toml = "[[group]]\nanchor = \"top-left\"\nwidgets = [\"news\", \"face\", \"cpu\"]\n\
                [widget.news]\ntype = \"ticker\"\nsource = \"news\"\n\
                [widget.face]\ntype = \"nul\"\nsource = \"cpu\"\n\
                [widget.cpu]\nlabel = \"C\\u0000PU\"\n";
    std::fs::write(conf.join("retro.toml"), toml).unwrap();

    let mut wall = Wall::start(&x, &home, &[]);
    assert!(wall.wait_for("textures, made in", 20.0), "the wall did not start:\n{}", wall.all_said());
    assert!(eventually(10.0, || home.join("news.pid").exists() && home.join("face.pid").exists()), "the plugins did not run:\n{}", wall.all_said());
    // a ticker draws its text once the reading is in, and the source runs every 2 s
    std::thread::sleep(Duration::from_secs(5));
    let said = wall.all_said();
    assert!(wall.alive(), "it stopped:\n{said}");
    assert!(!said.contains("faces/nul"), "the face's text was refused, not drawn:\n{said}");
    let _ = std::fs::remove_dir_all(&home);
}

/// A snapshot waits for the faces' first answers: a drawing is in it, and a rejected answer is said.
#[test]
fn a_snapshot_shows_the_faces_first_answers() {
    let Some(x) = Xvfb::start("a_snapshot_shows_the_faces_first_answers", 640, 360) else { return };
    let home = scratch("faces");
    let conf = home.join(".config/retro");
    for (name, body) in [
        ("red", r##"while read -r _; do sleep 0.5; echo '[{"op": "rect", "x": 0, "y": 0, "w": 4096, "h": 4096}, {"op": "fill", "colour": "#ff0000"}]'; done"##),
        ("notjson", "while read -r _; do sleep 0.5; echo '{[}'; done"),
    ] {
        let p = conf.join("faces").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let toml = "[[group]]\nanchor = \"top-left\"\nwidgets = [\"a\", \"b\"]\n[widget.a]\ntype = \"red\"\n[widget.b]\ntype = \"notjson\"\n";
    std::fs::write(conf.join("retro.toml"), toml).unwrap();
    let out = home.join("out.png");
    let run = x.retro_wall(&home).args(["--snapshot", out.to_str().unwrap(), "--theme", "phosphor", "--host", "testhost"]).output().unwrap();
    let said = String::from_utf8_lossy(&run.stderr).into_owned() + &String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success() && out.is_file(), "{said}");
    assert!(said.contains("not a list of operations"), "{said}");
    let (_, _, px) = common::x::pixels(&out);
    let red = px.chunks(4).filter(|p| p[2] > 200 && p[1] < 40 && p[0] < 40).count();
    assert!(red > 1000, "the red face is not in the snapshot ({red} red pixels):\n{said}");
    let _ = std::fs::remove_dir_all(&home);
}

/// A theme's shader may name its own variables as it likes, `t` and `c` too, on either profile.
#[test]
fn a_shader_names_its_variables_freely() {
    let Some(x) = Xvfb::start("a_shader_names_its_variables_freely", 320, 180) else { return };
    let home = scratch("shader-names");
    let theme = home.join("t");
    std::fs::create_dir_all(&theme).unwrap();
    let fragment = "uniform float view;\nvoid main() {\n    float t = uv.x * 0.0 + view;\n    vec4 c = vec4(1.0, t, 0.0, 1.0);\n    gl_FragColor = c;\n}\n";
    std::fs::write(theme.join("red.frag"), fragment).unwrap();
    std::fs::write(theme.join("theme.toml"), "extends = \"phosphor\"\nscene = [\"sky\", \"red\"]\n[element.red]\ntype = \"shader\"\nfragment = \"red.frag\"\n")
        .unwrap();
    for gl in ["", "2.1"] {
        let out = home.join(format!("out{gl}.png"));
        let mut c = x.retro_wall(&home);
        if !gl.is_empty() {
            c.env("RETRO_GL", gl);
        }
        let run = c.args(["--snapshot", out.to_str().unwrap(), "--theme", theme.to_str().unwrap(), "--host", "-"]).output().unwrap();
        let said = String::from_utf8_lossy(&run.stderr).into_owned() + &String::from_utf8_lossy(&run.stdout);
        assert!(run.status.success() && !said.contains("left out"), "{gl}: {said}");
        let (_, _, px) = common::x::pixels(&out);
        let red = px.chunks(4).filter(|p| p[2] > 200 && p[1] < 40 && p[0] < 40).count();
        assert!(red > px.len() / 8, "{gl}: the shader did not draw ({red} red pixels):\n{said}");
    }
    let _ = std::fs::remove_dir_all(&home);
}

/// A theme whose shader is /dev/zero, a FIFO or a file too large, and whose texture is /dev/zero:
/// each refused at once and said; the snapshot is drawn without them.
#[test]
fn theme_files_that_are_not_regular_or_too_large_are_refused() {
    let Some(x) = Xvfb::start("theme_files_that_are_not_regular_or_too_large_are_refused", 320, 180) else { return };
    let home = scratch("files");
    let theme = home.join("t");
    std::fs::create_dir_all(&theme).unwrap();
    let pipe = theme.join("pipe.frag");
    let c = std::ffi::CString::new(pipe.to_str().unwrap()).unwrap();
    // SAFETY: a NUL-terminated path that outlives the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    std::fs::write(theme.join("big.frag"), vec![b' '; retro_wall::files::Kind::Shader.limit() as usize + 1]).unwrap();
    let mut toml = String::from("extends = \"phosphor\"\nscene = [\"sky\", \"zero\", \"pipe\", \"big\", \"tex\"]\n");
    for (name, fragment, textures) in
        [("zero", "/dev/zero", "[]"), ("pipe", "pipe.frag", "[]"), ("big", "big.frag", "[]"), ("tex", "big.frag", "[\"/dev/zero\"]")]
    {
        toml += &format!("[element.{name}]\ntype = \"shader\"\nfragment = {fragment:?}\ntextures = {textures}\n");
    }
    std::fs::write(theme.join("theme.toml"), toml).unwrap();
    let out = home.join("out.png");
    let t0 = std::time::Instant::now();
    let run = x.retro_wall(&home).args(["--snapshot", out.to_str().unwrap(), "--theme", theme.to_str().unwrap(), "--host", "-"]).output().unwrap();
    let said = String::from_utf8_lossy(&run.stderr).into_owned() + &String::from_utf8_lossy(&run.stdout);
    assert!(t0.elapsed() < Duration::from_secs(30), "a theme file was read without end:\n{said}");
    assert!(run.status.success() && out.is_file(), "{said}");
    assert!(said.contains("/dev/zero: not a regular file"), "{said}");
    assert!(said.contains("pipe.frag: not a regular file"), "{said}");
    assert!(said.contains("the most is"), "{said}");
    let _ = std::fs::remove_dir_all(&home);
}
