//! The terminal: unbuffered keys, the screen size, the alternate screen,
//! and restoring all of it on exit or on a signal. See term.md.

use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by SIGINT or SIGTERM; the main loop exits at its next check.
pub static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// The terminal in viewer mode; dropping it restores the original state.
pub struct Term {
    orig: libc::termios,
}

impl Term {
    /// No echo, no line buffering, the alternate screen, the cursor hidden;
    /// None when standard input is not a terminal.
    pub fn open() -> Option<Term> {
        let mut t = MaybeUninit::<libc::termios>::uninit();
        // SAFETY: fd 0 is checked to be a terminal; the termios buffer is
        // valid for tcgetattr and initialised before it is read; the signal
        // handler only stores to an atomic.
        let orig = unsafe {
            if libc::isatty(0) == 0 || libc::tcgetattr(0, t.as_mut_ptr()) != 0 {
                return None;
            }
            let orig = t.assume_init();
            let mut raw = orig;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO);
            raw.c_cc[libc::VMIN] = 0;
            raw.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSANOW, &raw);
            let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            libc::signal(libc::SIGINT, handler);
            libc::signal(libc::SIGTERM, handler);
            orig
        };
        print!("\x1b[?1049h\x1b[?25l\x1b[H\x1b[2J");
        let _ = io::stdout().flush();
        Some(Term { orig })
    }

    /// Columns and rows of standard output, 80 by 24 when unknown.
    pub fn size() -> (usize, usize) {
        let mut ws = MaybeUninit::<libc::winsize>::uninit();
        // SAFETY: TIOCGWINSZ fills a winsize; the result is read only on success.
        let ok = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, ws.as_mut_ptr()) } == 0;
        if ok {
            // SAFETY: the ioctl succeeded and initialised it.
            let ws = unsafe { ws.assume_init() };
            if ws.ws_col > 0 && ws.ws_row > 0 {
                return (ws.ws_col as usize, ws.ws_row as usize);
            }
        }
        (80, 24)
    }

    /// Wait up to `ms` for a key. Escape sequences (arrows, function keys)
    /// come back as 0.
    pub fn key(ms: i32) -> Option<u8> {
        let mut pfd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for the call.
        if unsafe { libc::poll(&mut pfd, 1, ms) } <= 0 {
            return None;
        }
        let mut b = [0u8; 16];
        match io::stdin().read(&mut b) {
            Ok(n) if n > 0 => Some(if b[0] == 0x1b { 0 } else { b[0] }),
            _ => None,
        }
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        print!("\x1b[?25h\x1b[?1049l");
        let _ = io::stdout().flush();
        // SAFETY: restoring the attributes read in `open`.
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.orig);
        }
    }
}
