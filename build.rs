use std::process::Command;
use std::io;

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=formatting.txt.bash");

    let output = Command::new("bash")
        .arg("formatting.txt.bash")
        .output()?;

    if !output.status.success() {
        panic!(
            "Failed to run formatting.txt.bash: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    std::fs::write("formatting.txt", output.stdout)
}

