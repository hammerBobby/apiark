fn main() {
    #[cfg(windows)]
    {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        if let Err(e) = resource.compile() {
            println!("cargo:warning=Failed to embed Windows icon: {e}");
        }
    }
}
