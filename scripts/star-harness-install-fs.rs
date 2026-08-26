use std::env;
use std::ffi::{CString, OsStr};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::process::ExitCode;

fn move_no_replace(source: &OsStr, destination: &OsStr) -> io::Result<()> {
    let source = CString::new(source.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source contains NUL"))?;
    let destination = CString::new(destination.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "destination contains NUL"))?;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe {
        unsafe extern "C" {
            fn renameat2(
                olddirfd: i32,
                oldpath: *const std::ffi::c_char,
                newdirfd: i32,
                newpath: *const std::ffi::c_char,
                flags: u32,
            ) -> i32;
        }
        const AT_FDCWD: i32 = -100;
        const RENAME_NOREPLACE: u32 = 1;
        if renameat2(
            AT_FDCWD,
            source.as_ptr(),
            AT_FDCWD,
            destination.as_ptr(),
            RENAME_NOREPLACE,
        ) == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            fn renamex_np(
                from: *const std::ffi::c_char,
                to: *const std::ffi::c_char,
                flags: u32,
            ) -> i32;
        }
        const RENAME_EXCL: u32 = 0x0000_0004;
        if renamex_np(source.as_ptr(), destination.as_ptr(), RENAME_EXCL) == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
    {
        let _ = (source, destination);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic move-no-replace is unsupported on this platform",
        ))
    }
}

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: star-harness-install-fs <hard-link-no-replace|move-no-replace> <source> <destination>");
        return ExitCode::from(2);
    };
    let Some(source) = args.next() else {
        eprintln!("missing hard-link source");
        return ExitCode::from(2);
    };
    let Some(destination) = args.next() else {
        eprintln!("missing hard-link destination");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: star-harness-install-fs <hard-link-no-replace|move-no-replace> <source> <destination>");
        return ExitCode::from(2);
    }

    let operation = if command == "hard-link-no-replace" {
        fs::hard_link(source, destination)
    } else if command == "move-no-replace" {
        move_no_replace(&source, &destination)
    } else {
        eprintln!("unknown filesystem operation");
        return ExitCode::from(2);
    };
    match operation {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("filesystem operation failed: {error}");
            ExitCode::FAILURE
        }
    }
}
