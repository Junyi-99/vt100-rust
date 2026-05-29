// End-to-end regression test for the VS16 emoji-width bug, driving a real
// tmux through a PTY.
//
// tmux is itself a terminal emulator: it parses the pane's byte stream with
// its own width tables (which count an emoji-presentation sequence as two
// columns) and then *re-renders* its grid to the attached client. We attach
// our `vt100::Parser` as that client. If vt100 disagrees with tmux about the
// width of "❤️" (U+2764 U+FE0F), every column after the emoji is shifted by
// one in vt100's reconstruction — the on-screen residue deck was seeing.
//
// This test makes tmux print "AB❤️CD" and asserts that vt100 reconstructs the
// emoji as a wide cell with the following columns correctly aligned. It is
// skipped (not failed) when tmux is not installed.

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn have_tmux() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn tmux_renders_vs16_emoji_as_wide_cell() {
    if !have_tmux() {
        eprintln!("tmux not available; skipping tmux integration test");
        return;
    }

    let (rows, cols) = (10u16, 40u16);
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    // Private server socket so we never touch the user's tmux sessions.
    let sock = format!("vt100emoji{}", std::process::id());
    let mut cmd = CommandBuilder::new("tmux");
    cmd.arg("-L");
    cmd.arg(&sock);
    cmd.arg("-f");
    cmd.arg("/dev/null"); // no user config
    cmd.arg("new-session");
    cmd.arg("-x");
    cmd.arg(cols.to_string());
    cmd.arg("-y");
    cmd.arg(rows.to_string());
    cmd.arg("sh");
    cmd.arg("-c");
    // "AB" + ❤️ (U+2764 U+FE0F, octal-escaped UTF-8) + "CD", then hold open.
    cmd.arg("printf 'AB\\342\\235\\244\\357\\270\\217CD'; sleep 30");
    cmd.env("TERM", "xterm-256color");
    cmd.env("LANG", "en_US.UTF-8");
    cmd.env("LC_ALL", "en_US.UTF-8");

    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();

    // tmux output is read on a thread; the master read is blocking.
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut parser = vt100::Parser::new(rows, cols, 0);
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut emoji_col: Option<u16> = None;
    while Instant::now() < deadline {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
            parser.process(&chunk);
        }
        let screen = parser.screen();
        for col in 0..cols {
            if screen
                .cell(0, col)
                .is_some_and(|c| c.contents().starts_with('\u{2764}'))
            {
                emoji_col = Some(col);
                break;
            }
        }
        if emoji_col.is_some() {
            break;
        }
    }

    // Tear down before asserting, so a failure can't leak a tmux server.
    child.kill().ok();
    let _ = std::process::Command::new("tmux")
        .args(["-L", &sock, "kill-server"])
        .output();

    let screen = parser.screen();
    let dump: String = (0..8)
        .filter_map(|c| screen.cell(0, c))
        .map(|c| {
            format!(
                "[{:?} w{} c{}]",
                c.contents(),
                u8::from(c.is_wide()),
                u8::from(c.is_wide_continuation())
            )
        })
        .collect();

    let col = emoji_col.unwrap_or_else(|| {
        panic!("tmux never rendered the emoji on row 0; row0 = {dump}")
    });

    let emoji = screen.cell(0, col).unwrap();
    assert!(
        emoji.is_wide(),
        "tmux-rendered ❤️ must occupy two columns; row0 = {dump}"
    );
    assert!(
        screen.cell(0, col + 1).unwrap().is_wide_continuation(),
        "cell after ❤️ must be a wide continuation; row0 = {dump}"
    );
    assert_eq!(
        screen.cell(0, col + 2).unwrap().contents(),
        "C",
        "C must sit two columns after the emoji base (no drift); row0 = {dump}"
    );
    assert_eq!(
        screen.cell(0, col + 3).unwrap().contents(),
        "D",
        "D must follow C; row0 = {dump}"
    );
}
