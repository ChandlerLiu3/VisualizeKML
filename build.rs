use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if !env::var("TARGET").is_ok_and(|target| target.contains("windows")) {
        return;
    }

    for name in ["WindowsSdkDir", "WindowsSDKVersion", "ProgramFiles(x86)"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let package_version = env::var("CARGO_PKG_VERSION").expect("Cargo sets package version");
    let version_parts: Vec<u16> = package_version
        .split('.')
        .map(|part| {
            part.parse()
                .unwrap_or_else(|_| panic!("invalid numeric package version: {package_version}"))
        })
        .collect();
    assert_eq!(
        version_parts.len(),
        3,
        "Windows file version requires a three-component package version"
    );

    let numeric_version = format!(
        "{}, {}, {}, 0",
        version_parts[0], version_parts[1], version_parts[2]
    );
    let file_version = package_version;
    let resource_source = out_dir.join("visualize_kml_version.rc");
    let resource_binary = out_dir.join("visualize_kml_version.res");
    fs::write(
        &resource_source,
        format!(
            r#"1 VERSIONINFO
FILEVERSION {numeric_version}
PRODUCTVERSION {numeric_version}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "VisualizeKML\0"
      VALUE "FileDescription", "VisualizeKML KML trajectory viewer\0"
      VALUE "FileVersion", "{file_version}\0"
      VALUE "InternalName", "visualize_kml\0"
      VALUE "OriginalFilename", "visualize_kml.exe\0"
      VALUE "ProductName", "VisualizeKML\0"
      VALUE "ProductVersion", "{file_version}\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
        ),
    )
    .expect("write Windows version resource source");

    let resource_compiler = find_resource_compiler();
    let status = Command::new(&resource_compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&resource_binary)
        .arg(&resource_source)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "could not run Windows resource compiler {}: {error}",
                resource_compiler.display()
            )
        });
    assert!(
        status.success(),
        "Windows resource compiler failed with {status}"
    );
    println!(
        "cargo:rustc-link-arg-bin=visualize_kml={}",
        resource_binary.display()
    );
}

fn find_resource_compiler() -> PathBuf {
    let command_name = OsStr::new("rc.exe");
    if Command::new(command_name).arg("/?").output().is_ok() {
        return PathBuf::from(command_name);
    }

    let mut sdk_roots = Vec::new();
    if let Some(root) = env::var_os("WindowsSdkDir") {
        sdk_roots.push(PathBuf::from(root));
    }
    if let Some(program_files) = env::var_os("ProgramFiles(x86)") {
        sdk_roots.push(PathBuf::from(program_files).join("Windows Kits/10"));
    }

    let architecture = match env::var("TARGET").as_deref() {
        Ok(target) if target.starts_with("x86_64-") => "x64",
        Ok(target) if target.starts_with("i686-") => "x86",
        Ok(target) if target.starts_with("aarch64-") => "arm64",
        _ => panic!("unsupported Windows target architecture for resource compilation"),
    };
    for root in sdk_roots {
        if let Some(version) = env::var_os("WindowsSDKVersion") {
            let candidate = root
                .join("bin")
                .join(version)
                .join(architecture)
                .join("rc.exe");
            if candidate.is_file() {
                return candidate;
            }
        }
        if let Ok(entries) = fs::read_dir(root.join("bin")) {
            let mut versions: Vec<_> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            versions.sort();
            for version in versions.into_iter().rev() {
                let candidate = version.join(architecture).join("rc.exe");
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    panic!("Windows SDK resource compiler rc.exe was not found");
}
