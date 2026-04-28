use anyhow::{anyhow, bail, Context, Result};
use std::ffi::OsStr;
use std::process::Command;

pub trait CommandRunner {
    fn output(&self, program: &str, args: &[&str]) -> Result<String>;
    fn status(&self, program: &str, args: &[&str]) -> Result<()>;
}

#[derive(Debug, Copy, Clone)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn output(&self, program: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(program)
            .args(args.iter().map(OsStr::new))
            .output()
            .with_context(|| format!("failed to execute {} {}", program, args.join(" ")))?;

        if !output.status.success() {
            bail!(
                "{} {} failed: {}",
                program,
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        String::from_utf8(output.stdout).map_err(|e| anyhow!(e))
    }

    fn status(&self, program: &str, args: &[&str]) -> Result<()> {
        let status = Command::new(program)
            .args(args.iter().map(OsStr::new))
            .status()
            .with_context(|| format!("failed to execute {} {}", program, args.join(" ")))?;

        if status.success() {
            Ok(())
        } else {
            bail!("{} {} failed with {}", program, args.join(" "), status)
        }
    }
}
