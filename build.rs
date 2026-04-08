fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=resources/app.ico");

        let icon_path = std::path::Path::new("resources/app.ico");
        if icon_path.exists() {
            let mut resources = winres::WindowsResource::new();
            resources.set_icon(icon_path.to_string_lossy().as_ref());
            resources
                .compile()
                .expect("failed to compile Windows resources");
        }
    }
}
