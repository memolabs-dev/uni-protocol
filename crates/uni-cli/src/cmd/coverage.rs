use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    intent: String,
    #[serde(default)]
    criteria: Vec<Criterion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Criterion {
    id: String,
    claim: String,
    ensure: String,
    verifier: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Finding {
    pub criterion: String,
    pub kind: &'static str,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Coverage {
    pub status: &'static str,
    pub declared: usize,
    pub mapped: usize,
    pub findings: Vec<Finding>,
    pub authority: &'static str,
    pub scope: &'static str,
    pub manifest_sha256: String,
    pub revision_pin: &'static str,
}

impl Coverage {
    pub fn complete(&self) -> bool {
        self.findings.is_empty()
    }
}

fn problem(
    findings: &mut Vec<Finding>,
    criterion: &str,
    kind: &'static str,
    detail: impl Into<String>,
) {
    findings.push(Finding {
        criterion: criterion.to_owned(),
        kind,
        detail: detail.into(),
    });
}

/// Only audits the *declared* acceptance scope, not semantic verification
/// of the owner's outcome or ownership/authenticity of this manifest.
pub(crate) fn analyze(
    contract: &uni_ir::Ir,
    registry: &HashMap<String, uni_verify::VerifierSpec>,
    manifest_path: &Path,
) -> Result<Coverage> {
    let contents = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("read acceptance manifest {}", manifest_path.display()))?;
    let manifest: Manifest = toml::from_str(&contents)
        .with_context(|| format!("parse acceptance manifest {}", manifest_path.display()))?;
    let mut findings = vec![];
    let digest = uni_evidence::sha256_hex(contents.as_bytes());
    // Optional revision pin is provided by the caller environment; only
    // external access control can establish that it comes from an owner.
    let revision_pin = match std::env::var("UNI_ACCEPTANCE_SHA256") {
        Err(std::env::VarError::NotPresent) => "UNPINNED",
        Err(std::env::VarError::NotUnicode(_)) => {
            problem(
                &mut findings,
                "<manifest>",
                "invalid_revision_pin",
                "UNI_ACCEPTANCE_SHA256 must be 64 hexadecimal characters",
            );
            "INVALID"
        }
        Ok(expected)
            if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            problem(
                &mut findings,
                "<manifest>",
                "invalid_revision_pin",
                "UNI_ACCEPTANCE_SHA256 must be 64 hexadecimal characters",
            );
            "INVALID"
        }
        Ok(expected) if !expected.eq_ignore_ascii_case(&digest) => {
            problem(
                &mut findings,
                "<manifest>",
                "manifest_digest_mismatch",
                format!("manifest sha256:{digest} differs from the configured pin"),
            );
            "MISMATCH"
        }
        Ok(_) => "MATCHED",
    };
    if manifest.version != 1 {
        problem(
            &mut findings,
            "<manifest>",
            "unsupported_version",
            format!("expected version = 1, got {}", manifest.version),
        );
    }
    if manifest.intent != contract.intent.id {
        problem(
            &mut findings,
            "<manifest>",
            "intent_mismatch",
            format!(
                "manifest intent '{}' differs from contract '{}'",
                manifest.intent, contract.intent.id
            ),
        );
    }
    if manifest.criteria.is_empty() {
        problem(
            &mut findings,
            "<manifest>",
            "empty_manifest",
            "no owner acceptance criteria declared",
        );
    }
    let mut ids = HashSet::new();
    let mut assigned_claims = HashSet::new();
    let mut mapped = 0;
    for criterion in &manifest.criteria {
        let count = findings.len();
        let id = criterion.id.trim();
        if id.is_empty() || !ids.insert(id.to_owned()) {
            problem(
                &mut findings,
                id,
                "duplicate_or_empty_id",
                "criterion IDs must be unique and non-empty",
            );
        }
        let claim_id = criterion.claim.trim();
        if claim_id.is_empty() || !assigned_claims.insert(claim_id.to_owned()) {
            problem(
                &mut findings,
                id,
                "duplicate_or_empty_claim",
                "each criterion must map to a distinct, non-empty claim in v1",
            );
        }
        if criterion.ensure.trim().is_empty() {
            problem(
                &mut findings,
                id,
                "empty_ensure",
                "owner criterion text must not be empty",
            );
        }
        match contract.claims.iter().find(|c| c.id == claim_id) {
            None => problem(
                &mut findings,
                id,
                "missing_claim",
                format!("claim '{}' is missing from the contract", criterion.claim),
            ),
            Some(claim) => {
                if !(claim.required || claim.critical) {
                    problem(
                        &mut findings,
                        id,
                        "optional_claim",
                        format!("claim '{}' must be REQUIRED or CRITICAL", claim.id),
                    );
                }
                if claim.ensure.trim() != criterion.ensure.trim() {
                    problem(
                        &mut findings,
                        id,
                        "ensure_mismatch",
                        format!("claim '{}' ENSURE differs from acceptance text", claim.id),
                    );
                }
            }
        }
        let verify = contract
            .verification
            .iter()
            .find(|v| v.claim_id == claim_id && v.verifier_ref == criterion.verifier);
        match verify {
            None => problem(
                &mut findings,
                id,
                "missing_verifier",
                format!(
                    "claim '{}' has no VERIFY USING '{}'",
                    claim_id, criterion.verifier
                ),
            ),
            Some(v) if v.inline_shell.is_some() => problem(
                &mut findings,
                id,
                "inline_verifier",
                "acceptance scope must reference a trusted named registry verifier",
            ),
            Some(_) if !registry.contains_key(&criterion.verifier) => problem(
                &mut findings,
                id,
                "untrusted_verifier",
                format!(
                    "verifier '{}' is absent from .uni/config.toml",
                    criterion.verifier
                ),
            ),
            Some(_) => {}
        }
        if count == findings.len() {
            mapped += 1;
        }
    }
    Ok(Coverage {
        status: if findings.is_empty() {
            "COMPLETE"
        } else {
            "INCOMPLETE"
        },
        declared: manifest.criteria.len(),
        mapped,
        findings,
        authority: "CALLER_SUPPLIED_NOT_AUTHENTICATED",
        scope: "DECLARED_CRITERIA_ONLY",
        manifest_sha256: digest,
        revision_pin,
    })
}

pub(crate) fn print_coverage(coverage: &Coverage, as_json: bool) {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(coverage).expect("serializable")
        );
    } else {
        println!("UNI acceptance coverage: {}", coverage.status);
        println!(
            "Mapped {}/{} criteria; revision pin {}; manifest author not authenticated",
            coverage.mapped, coverage.declared, coverage.revision_pin
        );
        for finding in &coverage.findings {
            println!(
                "  [{}] {}: {}",
                finding.kind, finding.criterion, finding.detail
            );
        }
    }
}

pub(crate) fn cmd_coverage(
    contract_path: &Path,
    manifest_path: &Path,
    as_json: bool,
) -> Result<()> {
    let (ir, _) = crate::cmd::contract::load_contract(contract_path)?;
    let registry = uni_verify::load_registry(&crate::dot_uni());
    let coverage = analyze(&ir, &registry, manifest_path)?;
    print_coverage(&coverage, as_json);
    if !coverage.complete() {
        return Err(anyhow!("UNI acceptance coverage INCOMPLETE"));
    }
    Ok(())
}
