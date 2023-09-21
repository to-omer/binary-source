use std::{
    env::current_dir,
    fs,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

use anyhow::{ensure, Context as _, Result};
use bytesize::ByteSize;
use cargo_metadata::{
    camino::{Utf8Path, Utf8PathBuf},
    Metadata, MetadataCommand,
};
use data_encoding::{BASE64, BASE64_NOPAD, HEXUPPER};
use sha2::digest::Digest;
use structopt::StructOpt;

fn get_file_size(path: impl AsRef<Path>) -> Result<u64> {
    Ok(fs::metadata(path)?.len())
}

#[derive(Debug, StructOpt)]
struct Config {
    /// `cargo` Path to Cargo.toml
    #[structopt(long, value_name("PATH"))]
    manifest_path: Option<PathBuf>,

    /// Output filename
    #[structopt(long, short, value_name("PATH"), default_value = "main.rs")]
    output: PathBuf,

    /// Name of the bin target to compile
    #[structopt(long, value_name("NAME"))]
    bin: Option<String>,

    /// target
    #[structopt(long, value_name("TRIPLE"), default_value = "x86_64-unknown-linux-gnu")]
    target: String,

    /// Use `cross` to compile
    #[structopt(long)]
    use_cross: bool,

    /// If false, panic_abort
    #[structopt(long)]
    panic_unwind: bool,

    /// Do not add opt-level="s"
    #[structopt(long)]
    no_opt_size: bool,

    /// Do no use upx unless available
    #[structopt(long)]
    no_upx: bool,

    /// Output language [Rust|Python]
    #[structopt(long, default_value = "Rust")]
    language: Language,
}

#[derive(Debug, Default)]
enum Language {
    #[default]
    Rust,
    Python,
}

impl FromStr for Language {
    type Err = &'static str;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "rust" => Self::Rust,
            "python" => Self::Python,
            _ => Err("Could not parse Language")?,
        })
    }
}

struct Ctx<'a> {
    bin_name: &'a str,
    compile_dir: &'a Utf8Path,
    src_path: &'a Utf8PathBuf,
    binary_path: Utf8PathBuf,
}

impl Config {
    fn metadata(&self) -> Result<Metadata> {
        let cwd = current_dir().with_context(|| "Failed to get CWD")?;
        let mut cmd = MetadataCommand::new();
        if let Some(manifest_path) = &self.manifest_path {
            cmd.manifest_path(manifest_path);
        }
        Ok(cmd.current_dir(cwd).exec()?)
    }

    fn ctx<'a>(&self, metadata: &'a Metadata) -> Result<Ctx<'a>> {
        let package = metadata
            .root_package()
            .with_context(|| "Failed to find root package")?;
        let bin = {
            package
                .targets
                .iter()
                .find(|t| t.is_bin() && self.bin.as_ref().map_or(true, |b| b == &t.name))
                .with_context(|| "Failed to find bin")?
        };
        Ok(Ctx {
            bin_name: &bin.name,
            compile_dir: package
                .manifest_path
                .parent()
                .expect("`manifest_path` should end with \"Cargo.toml\""),
            src_path: &bin.src_path,
            binary_path: metadata
                .target_directory
                .join(&self.target)
                .join("release")
                .join(&bin.name),
        })
    }

    fn compile(&self, ctx: &Ctx<'_>) -> Result<()> {
        let mut cmd = Command::new(if self.use_cross { "cross" } else { "cargo" });
        cmd.arg("+nightly")
            .arg("build")
            .arg(format!("--target={}", self.target));
        if !self.panic_unwind {
            cmd.arg("-Zbuild-std=std,panic_abort")
                .arg("-Zbuild-std-features=panic_immediate_abort")
                .arg("--config=profile.release.panic=\"abort\"");
        }
        if !self.no_opt_size {
            cmd.arg("--config=profile.release.opt-level=\"s\"");
        }
        cmd.arg("--config=profile.release.codegen-units=1")
            .arg("--config=profile.release.lto=true")
            .arg("--config=profile.release.strip=true")
            .arg("--release")
            .arg("--bin")
            .arg(ctx.bin_name);
        let status = cmd.current_dir(ctx.compile_dir).status()?;
        ensure!(status.success(), "Build failed");
        Ok(())
    }

    fn compress(&self, ctx: &Ctx<'_>) -> Result<()> {
        let status = Command::new("upx")
            .args(["--best", "--lzma", "-qq"])
            .arg(&ctx.binary_path)
            .status()?;
        ensure!(status.success(), "upx failed");
        Ok(())
    }

    fn embed(&self, ctx: &Ctx<'_>) -> Result<String> {
        let template = match self.language {
            Language::Rust => include_str!("../data/binary_runner.rs.txt"),
            Language::Python => include_str!("../data/binary_runner.py.txt"),
        };
        let bin = fs::read(&ctx.binary_path)?;
        let b64 = match self.language {
            Language::Rust => BASE64_NOPAD,
            Language::Python => BASE64,
        };
        let bin_base64 = b64.encode(&bin);
        let hash = &HEXUPPER.encode(&sha2::Sha256::digest(&bin))[0..8];
        let ext = if self.target.split('-').nth(2) == Some("windows") {
            ".exe"
        } else {
            ""
        };
        let name = format!("bin{hash}{ext}");
        let source_code =
            fs::read_to_string(ctx.src_path).unwrap_or("SOURCE CODE NOT FOUND".to_string());

        let code = template
            .replacen("{{BINARY}}", &bin_base64, 1)
            .replacen("{{NAME}}", &name, 1)
            .replacen("{{SOURCE_CODE}}", source_code.trim_end(), 1);
        Ok(code)
    }

    fn gen_binary_source(&self) -> Result<String> {
        let metadata = self.metadata()?;
        let ctx = self.ctx(&metadata)?;
        self.compile(&ctx)?;
        let size = ByteSize::b(get_file_size(&ctx.binary_path)?);
        println!("Built binary size: {size}");

        if !self.no_upx {
            self.compress(&ctx)?;
            let size = ByteSize::b(get_file_size(&ctx.binary_path)?);
            println!("Compressed binary size: {size}");
        }

        let code = self.embed(&ctx)?;
        let size = ByteSize::b(code.len() as u64);
        println!("Bundled code size: {size}");

        Ok(code)
    }

    fn save_binary(&self, src: &[u8]) -> Result<()> {
        fs::write(&self.output, src)?;
        println!("Wrote code to `{}`", self.output.display());
        Ok(())
    }
}

fn main() -> Result<()> {
    let config = Config::from_args();
    let src = config.gen_binary_source()?;
    config.save_binary(src.as_bytes())
}
