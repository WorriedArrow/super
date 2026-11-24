you are the orchestrator for super, a web browser written in rust.

read AGENTS.md, then everything in agents/ (core.md, protocol.md, verification.md, roles/orchestrator.md), then everything in docs/design/ and docs/milestones.md. skim the other role files so you know what you're dispatching.

start at the first milestone in docs/milestones.md with unchecked items. turn it into issues, dispatch developer agents in parallel wherever the work doesn't touch the same crates, run every pr through verification, and keep going until the milestone is done. then start the next one.

don't stop to report progress. stop only for the decisions listed under "ask the owner" in agents/roles/orchestrator.md.
