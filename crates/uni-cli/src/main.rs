use anyhow::Result;
mod brief;
mod bundle;
mod cmd;
mod events;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(name = "uni", version, about = "Outcome Assurance Protocol CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    #[arg(long, global = true)]
    json: bool,
}

#[derive(clap::ValueEnum, Copy, Clone, Debug)]
pub(crate) enum InitProfile {
    Rust,
    #[value(alias = "nodejs")]
    Node,
    Python,
}

#[derive(Subcommand)]
enum Cmd {
    Init {
        /// Starter commands for trusted verifiers. Review .uni/config.toml
        /// before verifying a contract; missing project scripts must fail.
        #[arg(long, value_enum, default_value_t = InitProfile::Rust)]
        profile: InitProfile,
    },
    Compile {
        file: PathBuf,
    },
    Verify {
        file: PathBuf,
        /// Verifier actor identity (e.g. ci:build-12). Always self-declared:
        /// a flag names an actor, it never proves one. Only a JWT in
        /// UNI_IDENTITY_TOKEN, verified against a pinned issuer, reaches A3.
        #[arg(long)]
        actor: Option<String>,
        /// Reserved: signed provenance (A4) has no producer yet.
        #[arg(long, default_value_t = false)]
        attest: bool,
    },
    Explain {
        claim_or_intent: Option<String>,
        /// Emit GitHub Actions annotations for the drifted claims.
        #[arg(long, default_value_t = false)]
        annotations: bool,
    },
    Inspect {
        file: PathBuf,
    },
    ImportSpeckit {
        dir: PathBuf,
    },
    Report,
    Events {
        /// Read the retained archives as well as the current journal.
        #[arg(long)]
        all: bool,
        /// Emit an OTLP/JSON document (one span per event) for a collector.
        #[arg(long)]
        otlp: bool,
    },
    Lint {
        file: PathBuf,
    },
    Doctor,
    #[command(subcommand)]
    Pack(cmd::pack::PackCmd),
    /// Authorize a claim's resolution requirement to a concrete verifier
    /// (human act; AI may propose, only `uni bind` authorizes).
    Bind {
        /// Claim to authorize (with --verifier and, for templates, --selector).
        #[arg(long)]
        claim: Option<String>,
        #[arg(long)]
        verifier: Option<String>,
        /// Human-readable resolution requirement (optional for plain bindings).
        #[arg(long, default_value = "")]
        requirement: String,
        /// Concrete test selector for `{{selector}}` template verifiers.
        #[arg(long)]
        selector: Option<String>,
        /// Authorize every entry of a reviewed bindings file in one act.
        #[arg(long)]
        from: Option<PathBuf>,
    },
    /// List authorized verifier bindings.
    Bindings,
    /// Run a command in the workspace, then verify the contract: the execute
    /// half of the loop. UNI integrates no agent; you pass the command.
    Run {
        file: PathBuf,
        /// The command to run, as a shell command line (quote it yourself).
        #[arg(last = true, required = true)]
        command: Vec<String>,
        /// Verifier actor identity for the verification that follows.
        #[arg(long)]
        actor: Option<String>,
        /// Milliseconds to wait for the command before killing it.
        #[arg(long, default_value_t = 900_000)]
        timeout_ms: u64,
        /// Write the work order to .uni/brief.md and export UNI_BRIEF to the
        /// executor, so a command that was never told about it can still read it.
        #[arg(long)]
        brief: bool,
    },
    /// Emit the deterministic work order for an implementing agent
    /// (claims + the exact evidence each one requires). Guidance, not authority.
    Brief {
        file: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    #[command(subcommand)]
    Bundle(cmd::handoff::BundleCmd),
}

pub(crate) fn dot_uni() -> PathBuf {
    PathBuf::from(".uni")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init { profile } => cmd::contract::cmd_init(profile),
        Cmd::Compile { file } => cmd::contract::cmd_compile(&file, cli.json),
        Cmd::Verify {
            file,
            actor,
            attest,
        } => cmd::verify::cmd_verify(&file, cli.json, actor.as_deref(), attest),
        Cmd::Explain {
            claim_or_intent,
            annotations,
        } => cmd::report::cmd_explain(claim_or_intent, cli.json, annotations),
        Cmd::Inspect { file } => cmd::contract::cmd_inspect(&file, cli.json),
        Cmd::ImportSpeckit { dir } => cmd::speckit::cmd_import_speckit(&dir, cli.json),
        Cmd::Report => cmd::report::cmd_report(cli.json),
        Cmd::Events { all, otlp } => cmd::journal::cmd_events(cli.json, 50, all, otlp),
        Cmd::Lint { file } => cmd::contract::cmd_lint(&file, cli.json),
        Cmd::Doctor => cmd::journal::cmd_doctor(cli.json),
        Cmd::Pack(sub) => cmd::pack::cmd_pack(sub, cli.json),
        Cmd::Bind {
            claim,
            verifier,
            requirement,
            selector,
            from,
        } => cmd::authorize::cmd_bind(
            claim.as_deref(),
            verifier.as_deref(),
            &requirement,
            selector.as_deref(),
            from.as_deref(),
            cli.json,
        ),
        Cmd::Bindings => cmd::authorize::cmd_bindings(cli.json),
        Cmd::Bundle(sub) => cmd::handoff::cmd_bundle(sub, cli.json),
        Cmd::Brief { file, out } => cmd::handoff::cmd_brief(&file, out.as_deref(), cli.json),
        Cmd::Run {
            file,
            command,
            actor,
            timeout_ms,
            brief,
        } => cmd::verify::cmd_run(
            &file,
            &command.join(" "),
            actor.as_deref(),
            timeout_ms,
            brief,
            cli.json,
        ),
    }
}
