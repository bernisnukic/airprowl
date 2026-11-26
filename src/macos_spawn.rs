//! Make airprowl its own "responsible process" on macOS.
//!
//! macOS attributes privacy permissions (Bluetooth here) to the app that
//! launched a command-line tool, usually the terminal, and aborts the tool if
//! that app's Info.plist has no Bluetooth usage description. Terminal.app has
//! none, so a plain launch dies the moment CoreBluetooth starts.
//!
//! The fix: relaunch once with the responsibility disclaimed, so macOS treats
//! airprowl as its own app, reads the Info.plist that build.rs embeds in the
//! binary, and asks "airprowl would like to use Bluetooth". The parent just
//! waits and passes the child's exit status on. Debuggers and IDEs launch
//! their targets the same way.

use std::ffi::{CString, OsString, c_char, c_int};
use std::os::unix::ffi::OsStrExt;

const MARKER: &str = "AIRPROWL_RESPONSIBLE";

type SetDisclaim = unsafe extern "C" fn(*mut libc::posix_spawnattr_t, c_int) -> c_int;

/// Relaunch as a disclaimed child and exit with its status. Returns without
/// doing anything when already relaunched, or when the system lacks the call
/// (then the old behaviour applies: the terminal app is asked instead).
pub fn become_responsible() {
    if std::env::var_os(MARKER).is_some() || !needs_own_permissions() {
        return;
    }
    let Some(set_disclaim) = lookup_set_disclaim() else { return };
    let Ok(exe) = std::env::current_exe() else { return };

    let Some(path) = cstring(exe.as_os_str().as_bytes()) else { return };
    let Some(args) = std::env::args_os()
        .map(|a| cstring(a.as_bytes()))
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let mut env: Vec<CString> = std::env::vars_os()
        .filter_map(|(k, v)| cstring(env_pair(&k, &v).as_bytes()))
        .collect();
    env.push(CString::new(format!("{MARKER}=1")).expect("no NUL in marker"));

    let argv = null_terminated(&args);
    let envp = null_terminated(&env);

    // SAFETY: plain posix_spawn with an attribute object we own; argv/envp are
    // NULL-terminated arrays of pointers into CStrings that outlive the call.
    let status = unsafe {
        let mut attr: libc::posix_spawnattr_t = std::ptr::null_mut();
        if libc::posix_spawnattr_init(&mut attr) != 0 {
            return;
        }
        if set_disclaim(&mut attr, 1) != 0 {
            libc::posix_spawnattr_destroy(&mut attr);
            return;
        }
        let mut pid: libc::pid_t = 0;
        let rc = libc::posix_spawn(
            &mut pid,
            path.as_ptr(),
            std::ptr::null(),
            &attr,
            argv.as_ptr() as *const *mut c_char,
            envp.as_ptr() as *const *mut c_char,
        );
        libc::posix_spawnattr_destroy(&mut attr);
        if rc != 0 {
            return; // could not relaunch: carry on in this process
        }

        // Ctrl-C reaches the whole foreground process group; let the child
        // decide what it means and report back through its exit status.
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        let mut status: c_int = 0;
        while libc::waitpid(pid, &mut status, 0) == -1 {
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                std::process::exit(1);
            }
        }
        status
    };

    let code = if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else {
        1
    };
    std::process::exit(code);
}

/// Only the Bluetooth scanner needs this. As its own responsible app,
/// airprowl also needs its own permission for anything else privacy-guarded
/// (a binary kept on an external drive, for one), so other modes keep the
/// plain launch.
fn needs_own_permissions() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if ["-h", "--help", "-V", "--version", "--demo"].into_iter().any(has) {
        return false;
    }
    match args.iter().position(|a| a == "--scan") {
        Some(i) => matches!(
            args.get(i + 1).map(String::as_str),
            Some("bt" | "bluetooth")
        ),
        None => true, // the TUI runs every scanner
    }
}

/// `responsibility_spawnattrs_setdisclaim` is a private libSystem call, so it
/// is looked up at run time instead of linked.
fn lookup_set_disclaim() -> Option<SetDisclaim> {
    // SAFETY: dlsym with a static, NUL-terminated name; the symbol, when
    // present, has the signature declared in Apple's spawn_private.h.
    unsafe {
        let sym = libc::dlsym(libc::RTLD_DEFAULT, c"responsibility_spawnattrs_setdisclaim".as_ptr());
        (!sym.is_null()).then(|| std::mem::transmute::<*mut libc::c_void, SetDisclaim>(sym))
    }
}

fn env_pair(key: &std::ffi::OsStr, value: &std::ffi::OsStr) -> OsString {
    let mut pair = key.to_os_string();
    pair.push("=");
    pair.push(value);
    pair
}

fn cstring(bytes: &[u8]) -> Option<CString> {
    CString::new(bytes).ok()
}

fn null_terminated(strings: &[CString]) -> Vec<*const c_char> {
    strings.iter().map(|s| s.as_ptr()).chain(std::iter::once(std::ptr::null())).collect()
}
