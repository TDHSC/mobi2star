#![forbid(unsafe_code)]
use clap::{Parser, Subcommand};
use lexicon_core::{read_bounded, Error, LabelLanguage, Limits, Result, TargetReader};
use mobi2star::{Backend, OutputOptions, Profile};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    version,
    about = "Auditable MOBI dictionary to StarDict conversion; unsupported content is an error"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Emit machine-readable JSON (errors go to stderr)"
    )]
    json: bool,
    #[arg(long, global = true, default_value_t = 1024)]
    max_input_mib: usize,
    #[arg(long, global = true, default_value_t = 512)]
    max_text_mib: usize,
    #[arg(long, global = true, default_value_t = 32)]
    max_entry_mib: usize,
    #[arg(long, global = true, default_value_t = 8192)]
    max_output_mib: u64,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Read the container/header only. Passing preflight is NOT a conversion guarantee.
    Inspect { input: PathBuf },
    /// Convert, audit, reopen and verify, then publish OUTPUT/bundle. OUTPUT must not exist.
    Convert {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value_t = Backend::default())]
        backend: Backend,
        /// What to publish in OUTPUT/bundle
        #[arg(long, value_enum, default_value_t = Profile::default())]
        profile: Profile,
        #[arg(
            long,
            default_value_t = OutputOptions::default().offset_bits,
            help = "32 (portable default) or 64 (reader support required)"
        )]
        offset_bits: u8,
        #[arg(long, value_enum, default_value_t = OutputOptions::default().labels, help = "Language of generated chapter/supplement/gallery keys and offline viewer text")]
        labels: LabelLanguage,
        /// Reader to build for; decides how entries reference dictionary.css
        #[arg(long, value_enum, default_value_t = OutputOptions::default().reader)]
        reader: TargetReader,
    },
    /// Verify source identity, coverage, actual StarDict records, links, resources and provenance.
    Verify {
        bundle: PathBuf,
        #[arg(
            long,
            help = "Additionally require an exact match to this original source file"
        )]
        source: Option<PathBuf>,
    },
    /// Read every exact-spelling match, including homographs and explicit source aliases.
    Lookup { bundle: PathBuf, word: String },
}
fn mib(value: usize) -> Result<usize> {
    value
        .checked_mul(1024 * 1024)
        .filter(|&n| n > 0)
        .ok_or_else(|| Error::Limit("MiB limit must be nonzero and fit this platform".into()))
}
fn stdout_json(value: &impl serde::Serialize) -> Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer_pretty(&mut out, value)?;
    out.write_all(b"\n")?;
    Ok(())
}
fn run(cli: &Cli) -> Result<()> {
    let limits = Limits {
        input_bytes: mib(cli.max_input_mib)?,
        text_bytes: mib(cli.max_text_mib)?,
        entry_bytes: mib(cli.max_entry_mib)?,
        output_bytes: cli
            .max_output_mib
            .checked_mul(1024 * 1024)
            .filter(|&n| n > 0)
            .ok_or_else(|| Error::Limit("output byte limit".into()))?,
        ..Limits::default()
    };
    match &cli.command {
        Command::Inspect { input } => {
            let bytes = read_bounded(input, limits.input_bytes)?;
            let inspection = mobi_reader::inspect(&bytes)?;
            let detection = mobi_reader::Container::open(&bytes, &limits)
                .and_then(|m| m.source_archive().map(|a| a.map(|(n, _)| n)));
            let (backend, record, issue) = match detection {
                Ok(Some(n)) => ("srcs", Some(n), None),
                Ok(None) => ("compiled", None, None),
                Err(e) => ("blocked", None, Some(e.to_string())),
            };
            if cli.json {
                stdout_json(
                    &serde_json::json!({"inspection":inspection,"selected_backend":backend,"srcs_record":record,"format_error":issue,"scope":"header_only"}),
                )?;
            } else {
                let mut out = io::stdout().lock();
                writeln!(out,"{}\nPDB records: {}\nMOBI version: {}\nSelected backend: {}\nScope: header only; conversion performs the complete implemented checks.",inspection.header.metadata.title,inspection.records,inspection.header.version,backend)?;
                if let Some(reason) = issue {
                    writeln!(out, "Format error: {reason}")?;
                }
            }
        }
        Command::Convert {
            input,
            output,
            offset_bits,
            backend,
            profile,
            labels,
            reader,
        } => {
            let options = OutputOptions {
                offset_bits: *offset_bits,
                labels: *labels,
                reader: *reader,
            };
            let (bundle, report) = match profile {
                Profile::Bundle => {
                    mobi2star::convert_with_backend(input, output, &limits, options, *backend)?
                }
                Profile::Stardict => mobi2star::convert_dictionary(
                    input,
                    output,
                    &limits,
                    options,
                    *backend,
                    &mut |_| {},
                )?,
            };
            if cli.json {
                stdout_json(&serde_json::json!({"bundle": bundle, "report": report}))?;
            } else {
                writeln!(io::stdout().lock(), "Bundle: {}\nImplemented content checks: PASS\nHeadwords: {}, explicit aliases: {}, supplement entries: {}\nRendering: UNVERIFIED — acceptance-test in your reader.", bundle.display(), report.source_headwords(), report.source_aliases(), report.supplement_entries())?;
            }
        }
        Command::Verify { bundle, source } => {
            let report = mobi2star::verify_bundle(bundle, source.as_deref(), &limits)?;
            if cli.json {
                stdout_json(&report)?;
            } else {
                writeln!(io::stdout().lock(), "PASS: source-derived entries, aliases, coverage, file hashes and references.\nRendering remains unverified.")?;
            }
        }
        Command::Lookup { bundle, word } => {
            let dictionary_root = mobi2star::dictionary_root(bundle);
            let parsed = stardict_io::open(&dictionary_root, &limits)?;
            let targets = parsed.lookup(word);
            let mut file = std::fs::File::open(stardict_io::dictionary_file(&dictionary_root)?)?;
            let mut matches = Vec::new();
            for ordinal in targets {
                let entry = &parsed.entries[ordinal];
                let html = stardict_io::read_payload(&mut file, entry, limits.entry_bytes)?;
                matches.push(
                    serde_json::json!({"ordinal": ordinal, "headword": entry.word, "html": html}),
                );
            }
            if cli.json {
                stdout_json(&matches)?;
            } else {
                let mut out = io::stdout().lock();
                if matches.is_empty() {
                    writeln!(out, "No exact-spelling match.")?;
                }
                for entry in &matches {
                    writeln!(
                        out,
                        "<!-- entry {}: {} -->\n{}",
                        entry["ordinal"],
                        entry["headword"],
                        entry["html"].as_str().unwrap_or("")
                    )?;
                }
            }
        }
    }
    Ok(())
}
fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if cli.json {
                let value = serde_json::json!({"ok": false, "code": error.code(), "error": error.to_string()});
                let _ = writeln!(io::stderr().lock(), "{value}");
            } else {
                let _ = writeln!(io::stderr().lock(), "{}: {}", error.code(), error);
            }
            ExitCode::from(error.exit_code())
        }
    }
}
