fn main() {
    #[cfg(windows)]
    {
        let mut resource = winres::WindowsResource::new();
        // winres queries the registry via reg.exe to locate the Windows SDK,
        // which may be unavailable in restricted environments. Locate rc.exe
        // from the filesystem instead.
        if let Some(sdk_bin) = find_windows_sdk_bin() {
            resource.set_toolkit_path(sdk_bin.to_str().unwrap());
        }
        resource.set_icon("assets/icon.ico");
        if let Err(e) = resource.compile() {
            println!("cargo:warning=Failed to embed Windows icon: {e}");
        }
    }
}

#[cfg(windows)]
fn find_windows_sdk_bin() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    let program_files = std::env::var("ProgramFiles(x86)")
        .or_else(|_| std::env::var("ProgramFiles"))
        .unwrap_or_else(|_| r"C:\Program Files (x86)".to_string());
    let kits = PathBuf::from(program_files).join("Windows Kits").join("10");
    let bin = kits.join("bin");
    let entries = std::fs::read_dir(&bin).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    candidates.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for dir in candidates {
        let rc = dir.join("x64").join("rc.exe");
        if rc.exists() {
            return Some(dir.join("x64"));
        }
    }
    None
}
