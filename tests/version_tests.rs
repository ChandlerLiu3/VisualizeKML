use std::process::Command;

#[test]
fn executable_reports_its_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_visualize_kml"))
        .arg("--version")
        .output()
        .expect("application executable should run");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("version output should be UTF-8"),
        "VisualizeKML 0.3.0\n"
    );
}
