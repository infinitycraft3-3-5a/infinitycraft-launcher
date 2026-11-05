fn main() {
  println!("cargo:rerun-if-env-changed=IC_NO_UAC");

  // Base manifest identique au défaut Tauri (Common-Controls / visual styles).
  let base = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>"#;

  // Niveau d'exécution : requireAdministrator par défaut ;
  // asInvoker si IC_NO_UAC est défini au moment du build.
  let level = if std::env::var_os("IC_NO_UAC").is_some() {
    "asInvoker"
  } else {
    "requireAdministrator"
  };

  let trust_info = format!(
    "  <trustInfo xmlns=\"urn:schemas-microsoft-com:asm.v3\">\n    <security>\n      <requestedPrivileges>\n        <requestedExecutionLevel level=\"{level}\" uiAccess=\"false\" />\n      </requestedPrivileges>\n    </security>\n  </trustInfo>\n</assembly>"
  );

  let manifest = base.replace("</assembly>", &trust_info);

  let windows = tauri_build::WindowsAttributes::new().app_manifest(manifest);
  let attributes = tauri_build::Attributes::new().windows_attributes(windows);

  tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
