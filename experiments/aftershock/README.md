# Aftershock x UNI — read-only pilot

This is an **offline pilot** grounded in the real Aftershock code and its
SQLite schema in `guillaume-flambard/lab-infra/stacks/aftershock/aftershock.py`.
The pilot does **not** connect to the VPS or send email, modify notes or invoke
the model. It reads a disposable test database or an explicitly supplied
read-only SQLite **copy**.

## Why heartbeat does not equal a delivered result

Aftershock writes both `battement()` heartbeat entries and real `marquer()`
business cycles into the same `cycle` table (`vu_le,sujet,verdict,ecrit`).
A heartbeat has an empty subject. The service's real business interval defaults
to seven days; the `agent.yaml` dashboard observation window is 48 hours.
Checking `max(vu_le)` alone would mask weeks without real research and
applying the 48-hour heartbeat window to a weekly research cycle would generate
false alarms.

The observer therefore independently checks: a heartbeat within 48 hours,
a business cycle within an explicit eight-day pilot threshold, a recognized
review verdict (REJECT, REVISE, WATCH, TEST), and interpretable `ecrit` metadata.
A REVIEW/REJECT cycle that publishes no note remains legitimate. `ecrit=1`
does **not** prove a note is present on disk or that any research conclusions
are sound. Readability of the critic's actual objections, source verification,
cutoff date and corresponding note path are **not proved** by this metadata.
Those belong to the next pilot slice once correlated output evidence is
available. An unknown/missing database or missing cycle reports UNKNOWN and
exits nonzero, never success.

## Reproduce without runtime access

~~~sh
python3 -m unittest discover -s experiments/aftershock -p 'test_*.py'
python3 experiments/aftershock/observer.py --db /path/to/disposable/aftershock.db
~~~

The observer uses SQLite's read-only URI mode and cannot create tables or
initialize a missing database. Its JSON has `PASS`, `FAIL`, `UNKNOWN` states;
exit code 0 is reserved for PASS. No external network or API key is used.

The `contract.uni` file is the UNI claim for this pilot. To make it executable
in a *dedicated disposable workspace*, register the trusted verifier below in
the workspace's `.uni/config.toml` (adjust the paths to the test checkout and
an immutable SQLite snapshot, not the live DB):

~~~toml
[verifiers."aftershock.observer"]
type = "shell"
run = "python3 experiments/aftershock/observer.py --db snapshots/aftershock.db"
expect = "\"status\": \"PASS\""
max_age_hours = 1
~~~

Run `uni verify experiments/aftershock/contract.uni` from that workspace. The
verifier registry is trusted configuration, reviewed independently from agents.
Do not treat this CLI example as authorization to let an implementing agent
alter its own verifier command.

## Limits and follow-on

The observer deliberately avoids writing any output into the watched subject.
It does not verify which real note belongs to a cycle or whether the second
LLM call occurred independently. The live server has not been inspected here;
its SentinelX host is offline. First validate schema against a detached
read-only production *copy* and test source-to-note correlation; only then
consider a read-only agent observer adapter. No runtime changes or deployments.

## Cycle-to-note-to-source-to-critic traceability

Original Aftershock `cycle` rows contain no note filename or trace ID, and `dossier` has no cycle foreign key. Joining on subject and timestamp would invent a relationship. The optional `links.py` verifier therefore requires a separately reviewed JSON manifest (`uni.aftershock.links/v1`) naming each `cycle_id`, relative note path, SHA-256 of the exact note bytes, HTTPS source URLs, critic verdict and exact `dossier_vu_le` timestamp. See `links.uni` for the matching UNI claim.

Example synthetic manifest: `{"schema":"uni.aftershock.links/v1","all_cycles":true,"cycles":[{"cycle_id":7,"note":"cycle.md","sha256":"<digest>","source_urls":["https://example.org/a"],"critic_verdict":"WATCH","dossier_vu_le":"2026-10-12T14:02:01+00:00"}]}`. This is an illustration, not a claim about real cycle 7.

Run `python3 experiments/aftershock/links.py --db snapshots/aftershock.db --notes-root snapshots/notes --manifest snapshots/links.json` against detached copies only. The verifier checks note hashes, subject and critic verdict agreement, timestamp ordering, critic sections and non-placeholder content, declared URL occurrence, dossier uniqueness, and declared complete cycle coverage. `REJECT`/`REVISE` cycles legitimately may produce no note; they still require a dossier linkage. An ambiguous or missing link fails closed. URL occurrence does not validate source truth; critic text does not prove independent model execution. A future producer-side durable receipt with exact cycle, trace and note identifiers is needed for stronger assurance.
