use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_rc_exe() -> Option<PathBuf> {
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let kits_bin = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let entries = fs::read_dir(kits_bin).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("x64").join("rc.exe"))
        .filter(|p| p.is_file())
        .collect();
    candidates.sort();
    candidates.pop()
}

fn main() {
    println!("cargo:rerun-if-changed=assets/velvet.rc");
    println!("cargo:rerun-if-changed=assets/velvet.ico");

    let rc_script = Path::new("assets").join("velvet.rc");
    let ico_file = Path::new("assets").join("velvet.ico");
    if !rc_script.is_file() || !ico_file.is_file() {
        return;
    }

    let Some(rc_exe) = find_rc_exe() else {
        println!("cargo:warning=rc.exe not found; velvet.exe icon resource skipped");
        return;
    };
    let Ok(out_dir) = env::var("OUT_DIR") else {
        return;
    };
    let res_path = PathBuf::from(out_dir).join("velvet.res");

    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_script)
        .status();

    match status {
        Ok(st) if st.success() && res_path.is_file() => {
            println!("cargo:rustc-link-arg-bin=velvet={}", res_path.display());
        }
        _ => {
            println!("cargo:warning=rc.exe failed to compile assets/velvet.rc");
        }
    }
}
