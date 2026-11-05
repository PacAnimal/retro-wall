//! The scheduled task that starts the wall: `\retro-wall\retro-wall`, for the user who installed it, at
//! logon and kept running. Written as Task Scheduler's XML, which `schtasks /Create /XML` takes.
//!
//! Task Scheduler's restart on failure ignores exit codes, so a dead wall is brought back by repeating:
//! the logon trigger repeats every 5 minutes, and a start while the wall runs is ignored (IgnoreNew). A
//! logon trigger repeats only from a logon, so a second trigger repeats from the install on, for the
//! session the install is run in. The wall is started with no options: it reads them from the settings
//! (`settings`).

/// The task's folder and name, as schtasks takes them.
pub const NAME: &str = r"\retro-wall\retro-wall";

/// The task's XML: `program` started in `dir` for `user` (DOMAIN\name), the second trigger from
/// `from`, a local time as Task Scheduler writes it (2026-11-05T12:00:00).
pub fn xml(program: &str, dir: &str, user: &str, from: &str) -> String {
    let (program, dir, user, from) = (escape(program), escape(dir), escape(user), escape(from));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>retro-wall, the live wallpaper: started at logon, and brought back within 5 minutes if it stops.</Description>
    <URI>{NAME}</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT5S</Delay>
      <Repetition>
        <Interval>PT5M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
    </LogonTrigger>
    <TimeTrigger>
      <Enabled>true</Enabled>
      <StartBoundary>{from}</StartBoundary>
      <Repetition>
        <Interval>PT5M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>6</Priority>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{program}</Command>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// The XML as schtasks reads a file best: UTF-16, little-endian, with its byte order mark.
pub fn utf16(xml: &str) -> Vec<u8> {
    std::iter::once(0xfeffu16).chain(xml.encode_utf16()).flat_map(u16::to_le_bytes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_starts_the_wall_with_no_options_at_logon_and_keeps_it_running() {
        let x =
            xml(r"C:\Users\Ann & Bo\AppData\Local\retro-wall\retro-wall.exe", r"C:\Users\Ann & Bo\AppData\Local\retro-wall", r"PC\Ann", "2026-11-05T12:00:00");
        // what is not plain XML is escaped: a path with an ampersand
        assert!(x.contains(r"<Command>C:\Users\Ann &amp; Bo\AppData\Local\retro-wall\retro-wall.exe</Command>"));
        assert!(!x.contains("<Arguments>"), "the wall reads its options from its settings");
        for want in [
            "<LogonTrigger>",
            "<Interval>PT5M</Interval>",
            "<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<RunLevel>LeastPrivilege</RunLevel>",
            "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
            r"<UserId>PC\Ann</UserId>",
        ] {
            assert!(x.contains(want), "{want}");
        }
        let bytes = utf16(&x);
        assert_eq!(&bytes[..2], &[0xff, 0xfe], "UTF-16LE with its mark");
        assert_eq!(bytes.len(), 2 + 2 * x.encode_utf16().count());
    }
}
