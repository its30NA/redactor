//! `scrub clipboard`: sanitize the clipboard text in place (Windows).
//!
//! No Win32 bindings (this crate forbids `unsafe`): one PowerShell helper talks to the
//! clipboard through .NET, for the whole run.
//!
//! 1. It checks the password-manager marker (`ExcludeClipboardContentFromMonitorProcessing`):
//!    marked content is never read and never rewritten.
//! 2. It reads the text and sends it to scrub on stdout (raw UTF-8, length-prefixed).
//! 3. It waits on stdin for the redacted text (or for stdin to close: nothing to write).
//! 4. It reads the clipboard again and writes only if it still holds exactly the text it read,
//!    so a copy made meanwhile (a fresh password-manager copy) is never overwritten. The
//!    comparison happens inside the helper: no text, hash or fingerprint of the old content
//!    crosses back, and no text is ever passed on a command line.
//!
//! This does not remove the raw item from Windows clipboard history (Win+V); the desktop app
//! does that.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use redactor_core::{Engine, ScanRequest};

/// What the clipboard holds.
#[derive(Debug, PartialEq, Eq)]
pub enum ClipRead {
    Text(String),
    /// No text (empty, an image, files).
    Empty,
    /// Marked by its source as private; must not be touched.
    Protected,
}

/// How a clipboard session ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Finish {
    Written,
    /// The clipboard no longer held the text that was read: nothing was written.
    Changed,
    /// Nothing was asked to be written.
    Untouched,
}

/// One read-then-maybe-write session on the clipboard.
pub trait Clipboard {
    /// Reads the clipboard; the session stays open for [`Clipboard::finish`].
    fn read(&mut self) -> Result<ClipRead, String>;
    /// Writes `replacement` if the clipboard still holds the text that was read, and ends the
    /// session. `None` ends it without writing.
    fn finish(&mut self, replacement: Option<&str>) -> Result<Finish, String>;
}

/// The result of one clipboard run.
#[derive(Debug, PartialEq, Eq)]
pub enum ClipOutcome {
    Cleaned {
        removed: usize,
        summary: String,
    },
    Clean,
    /// No secrets, but invisible, bidi or tag characters were removed and written back.
    Stripped,
    Empty,
    Protected,
    /// Something else was copied during the scan; left untouched.
    Changed,
}

/// Reads the clipboard, redacts it, and writes it back only when the text changed (findings or
/// stripped hidden characters) and the clipboard still holds what was read.
pub fn scrub_clipboard(clip: &mut dyn Clipboard, engine: &Engine) -> Result<ClipOutcome, String> {
    let text = match clip.read()? {
        ClipRead::Text(t) if !t.is_empty() => t,
        other => {
            clip.finish(None)?;
            return Ok(match other {
                ClipRead::Protected => ClipOutcome::Protected,
                _ => ClipOutcome::Empty,
            });
        }
    };
    let outcome = engine.scan(&text, &ScanRequest::default());
    let rows = crate::audit::rows_of(&text, &outcome.findings);
    if outcome.output == text {
        clip.finish(None)?;
        return Ok(ClipOutcome::Clean);
    }
    // Written back whenever the text changed, as `scrub FILE` does: hidden characters alone
    // (no findings) are stripped too.
    Ok(match clip.finish(Some(&outcome.output))? {
        Finish::Written if rows.is_empty() => ClipOutcome::Stripped,
        Finish::Written => ClipOutcome::Cleaned {
            removed: rows.len(),
            summary: crate::audit::summary(&rows),
        },
        Finish::Changed | Finish::Untouched => ClipOutcome::Changed,
    })
}

/// The helper script. Single quotes only, so it passes through the command line unchanged.
const SCRIPT: &str = "\
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms
$c=[System.Windows.Forms.Clipboard]
$f=[System.Windows.Forms.TextDataFormat]::UnicodeText
$m='ExcludeClipboardContentFromMonitorProcessing'
if($c::ContainsData($m)){[Environment]::Exit(10)}
if(-not $c::ContainsText($f)){[Environment]::Exit(11)}
$t=$c::GetText($f)
$u=New-Object System.Text.UTF8Encoding($false,$true)
$b=$u.GetBytes($t)
$o=[Console]::OpenStandardOutput()
$h=[System.Text.Encoding]::ASCII.GetBytes([string]$b.Length+[char]10)
$o.Write($h,0,$h.Length)
$o.Write($b,0,$b.Length)
$o.Flush()
$i=[Console]::OpenStandardInput()
$n=0
while($true){$x=$i.ReadByte();if($x -lt 0){[Environment]::Exit(13)};if($x -eq 10){break};if($x -lt 48 -or $x -gt 57){[Environment]::Exit(14)};$n=$n*10+($x-48)}
$r=New-Object byte[] $n
$k=0
while($k -lt $n){$z=$i.Read($r,$k,$n-$k);if($z -le 0){[Environment]::Exit(14)};$k+=$z}
$s=$u.GetString($r)
if($c::ContainsData($m)){[Environment]::Exit(12)}
if(-not $c::ContainsText($f)){[Environment]::Exit(12)}
if(-not [string]::Equals($c::GetText($f),$t,[StringComparison]::Ordinal)){[Environment]::Exit(12)}
$c::SetText($s,$f)
[Environment]::Exit(0)";

const EXIT_WRITTEN: i32 = 0;
const EXIT_PROTECTED: i32 = 10;
const EXIT_NO_TEXT: i32 = 11;
const EXIT_CHANGED: i32 = 12;
const EXIT_NOTHING_TO_WRITE: i32 = 13;

/// Reads one length-prefixed frame (`<decimal length>\n<bytes>`); None at end of stream.
pub fn read_frame(r: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut header = Vec::new();
    if r.read_until(b'\n', &mut header)? == 0 {
        return Ok(None);
    }
    let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "bad frame header");
    let digits = header.strip_suffix(b"\n").ok_or_else(bad)?;
    let len: usize = std::str::from_utf8(digits)
        .ok()
        .and_then(|d| d.parse().ok())
        .ok_or_else(bad)?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Writes one length-prefixed frame.
pub fn write_frame(w: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    w.write_all(format!("{}\n", bytes.len()).as_bytes())?;
    w.write_all(bytes)?;
    w.flush()
}

/// The Windows clipboard through `powershell.exe -STA` (the clipboard needs an STA thread).
#[derive(Default)]
pub struct PowerShell {
    session: Option<Session>,
}

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Session {
    fn exit_code(mut self) -> Result<Option<i32>, String> {
        drop(self.stdin.take());
        self.child
            .wait()
            .map(|s| s.code())
            .map_err(|e| format!("clipboard helper failed: {e}"))
    }
}

impl Drop for PowerShell {
    fn drop(&mut self) {
        if let Some(mut s) = self.session.take() {
            drop(s.stdin.take());
            let _ = s.child.kill();
            let _ = s.child.wait();
        }
    }
}

impl Clipboard for PowerShell {
    fn read(&mut self) -> Result<ClipRead, String> {
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", SCRIPT])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start powershell.exe: {e}"))?;
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut session = Session {
            child,
            stdin,
            stdout,
        };
        match read_frame(&mut session.stdout) {
            Ok(Some(bytes)) => {
                self.session = Some(session);
                String::from_utf8(bytes)
                    .map(ClipRead::Text)
                    .map_err(|_| "clipboard read failed (not UTF-8)".to_string())
            }
            Ok(None) => match session.exit_code()? {
                Some(EXIT_PROTECTED) => Ok(ClipRead::Protected),
                Some(EXIT_NO_TEXT) => Ok(ClipRead::Empty),
                other => Err(format!("clipboard read failed (powershell exit {other:?})")),
            },
            Err(e) => Err(format!("clipboard read failed: {e}")),
        }
    }

    fn finish(&mut self, replacement: Option<&str>) -> Result<Finish, String> {
        let Some(mut session) = self.session.take() else {
            return Ok(Finish::Untouched);
        };
        if let Some(text) = replacement {
            let stdin = session.stdin.as_mut().ok_or("no stdin")?;
            write_frame(stdin, text.as_bytes())
                .map_err(|e| format!("clipboard write failed: {e}"))?;
        }
        match session.exit_code()? {
            Some(EXIT_WRITTEN) if replacement.is_some() => Ok(Finish::Written),
            Some(EXIT_CHANGED) => Ok(Finish::Changed),
            Some(EXIT_NOTHING_TO_WRITE) if replacement.is_none() => Ok(Finish::Untouched),
            other => Err(format!(
                "clipboard write failed (powershell exit {other:?})"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redactor_core::EngineConfig;

    struct Fake {
        held: ClipRead,
        /// Another copy lands between the read and the write.
        changes_meanwhile: bool,
        written: Vec<String>,
        finished: usize,
    }

    impl Fake {
        fn new(held: ClipRead) -> Fake {
            Fake {
                held,
                changes_meanwhile: false,
                written: vec![],
                finished: 0,
            }
        }
    }

    impl Clipboard for Fake {
        fn read(&mut self) -> Result<ClipRead, String> {
            Ok(match &self.held {
                ClipRead::Text(t) => ClipRead::Text(t.clone()),
                ClipRead::Empty => ClipRead::Empty,
                ClipRead::Protected => ClipRead::Protected,
            })
        }
        fn finish(&mut self, replacement: Option<&str>) -> Result<Finish, String> {
            self.finished += 1;
            match replacement {
                None => Ok(Finish::Untouched),
                Some(_) if self.changes_meanwhile => Ok(Finish::Changed),
                Some(t) => {
                    self.written.push(t.to_string());
                    Ok(Finish::Written)
                }
            }
        }
    }

    fn engine() -> Engine {
        Engine::new(EngineConfig::default()).unwrap()
    }

    fn password_line() -> String {
        format!(
            "DB_PASSWORD={}",
            ["Plum", "tree", "47", "-", "Kite"].concat()
        )
    }

    #[test]
    fn findings_are_written_back_cleaned() {
        let mut f = Fake::new(ClipRead::Text(password_line()));
        let out = scrub_clipboard(&mut f, &engine()).unwrap();
        assert!(
            matches!(out, ClipOutcome::Cleaned { removed: 1, .. }),
            "{out:?}"
        );
        assert_eq!(f.written, ["DB_PASSWORD=[REDACTED: Password]"]);
        assert_eq!(f.finished, 1);
    }

    #[test]
    fn a_copy_made_during_the_scan_is_left_alone() {
        let mut f = Fake::new(ClipRead::Text(password_line()));
        f.changes_meanwhile = true;
        assert_eq!(
            scrub_clipboard(&mut f, &engine()).unwrap(),
            ClipOutcome::Changed
        );
        assert!(f.written.is_empty());
    }

    #[test]
    fn clean_text_is_not_rewritten() {
        let mut f = Fake::new(ClipRead::Text("hello".into()));
        assert_eq!(
            scrub_clipboard(&mut f, &engine()).unwrap(),
            ClipOutcome::Clean
        );
        assert!(f.written.is_empty());
        assert_eq!(f.finished, 1, "the session is always ended");
    }

    #[test]
    fn hidden_characters_alone_are_stripped_like_scrub_file_does() {
        let mut f = Fake::new(ClipRead::Text("pay\u{202E}load\u{200B}".into()));
        assert_eq!(
            scrub_clipboard(&mut f, &engine()).unwrap(),
            ClipOutcome::Stripped
        );
        assert_eq!(f.written, ["payload"]);
    }

    #[test]
    fn protected_content_is_never_touched() {
        let mut f = Fake::new(ClipRead::Protected);
        assert_eq!(
            scrub_clipboard(&mut f, &engine()).unwrap(),
            ClipOutcome::Protected
        );
        assert!(f.written.is_empty());
    }

    #[test]
    fn no_text_is_reported_as_empty() {
        let mut f = Fake::new(ClipRead::Empty);
        assert_eq!(
            scrub_clipboard(&mut f, &engine()).unwrap(),
            ClipOutcome::Empty
        );
        let mut f = Fake::new(ClipRead::Text(String::new()));
        assert_eq!(
            scrub_clipboard(&mut f, &engine()).unwrap(),
            ClipOutcome::Empty
        );
    }

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, "grüße\nline two".as_bytes()).unwrap();
        write_frame(&mut buf, b"").unwrap();
        let mut r = std::io::Cursor::new(buf);
        assert_eq!(
            read_frame(&mut r).unwrap().unwrap(),
            "grüße\nline two".as_bytes()
        );
        assert_eq!(read_frame(&mut r).unwrap().unwrap(), b"");
        assert_eq!(read_frame(&mut r).unwrap(), None);
        assert!(read_frame(&mut std::io::Cursor::new(b"x\n".to_vec())).is_err());
        assert!(read_frame(&mut std::io::Cursor::new(b"9\nshort".to_vec())).is_err());
    }

    #[test]
    fn the_script_checks_the_marker_first_and_again_before_writing() {
        let marker = SCRIPT.find("ContainsData($m)").unwrap();
        let read = SCRIPT.find("GetText($f)").unwrap();
        assert!(marker < read);
        let set = SCRIPT.find("SetText(").unwrap();
        let recheck = SCRIPT.rfind("ContainsData($m)").unwrap();
        let compare = SCRIPT.find("[string]::Equals($c::GetText($f),$t").unwrap();
        assert!(read < recheck && recheck < compare && compare < set);
        assert!(SCRIPT.contains("$m='ExcludeClipboardContentFromMonitorProcessing'"));
        // Nothing the Windows command line could re-quote.
        assert!(!SCRIPT.contains('"'));
    }

    /// PowerShell's own parser accepts the script (parsed only, never run: the clipboard is
    /// not touched).
    #[cfg(windows)]
    #[test]
    fn the_script_parses() {
        let parse = "$s=[Console]::In.ReadToEnd();$t=$null;$e=$null;\
                     [void][System.Management.Automation.Language.Parser]::ParseInput($s,[ref]$t,[ref]$e);\
                     [Environment]::Exit($e.Count)";
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", parse])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(SCRIPT.as_bytes())
            .unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(0), "parse errors");
    }
}
