use serde_json::Value;
use std::{fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn text(path: &str) -> String {
    fs::read_to_string(root().join(path)).expect("UTF-8 published guidance must be readable")
}
fn flow() -> String {
    text("skills/agensi-operator/references/tools-and-workflows.md")
}
fn contains_all(document: &str, phrases: &[&str]) {
    for phrase in phrases {
        assert!(document.contains(phrase), "Missing guidance: {phrase}");
    }
}

#[test]
fn test_latest_use_without_acquired_files() {
    let flow = flow();
    contains_all(
        &flow,
        &[
            "Fetch `get_skill` again for every requested use",
            "Never save acquired instructions",
            "Use an owned skill again",
            "Buy an unowned $3",
            "Buy an $8",
            "Add credit, amount missing",
            "Post a review, rating missing",
            "Save acquired content or try old free allowance",
        ],
    );
    assert!(!flow.contains("Install free or owned one-time files"));
}
#[test]
fn test_exact_payment_and_topup_consent() {
    contains_all(
        &flow(),
        &[
            "exact slug, current price and payment route",
            "buy_credits",
            "5 to 1000",
            "No amount or approval: stop",
            "$3 one-time skill: credit only",
            "$8 one-time skill: card or credit",
            "Ask for specific approval before",
            "No `confirm=true` before specific approval",
            "credits cannot buy subscription skills",
        ],
    );
}
#[test]
fn test_rating_is_the_users_decision() {
    contains_all(
        &flow(),
        &[
            "review_skill",
            "user's own rating",
            "1 to 5",
            "Never invent or suggest a rating",
        ],
    );
}
#[test]
fn test_current_money_rights_and_no_trial_fallback() {
    contains_all(
        &text("skills/agensi-operator/references/policies.md"),
        &[
            "1 credit = $1",
            "12 months",
            "30 days",
            "70%",
            "$1",
            "Starter and Pro",
            "One connected agent",
            "subscription skills",
            "not a zero-persistence guarantee",
        ],
    );
    contains_all(&flow(), &["Do not use the retired free-run allowance"]);
}
#[test]
fn test_original_authoring_is_not_marketplace_installation() {
    contains_all(
        &text("README.md"),
        &[
            "original or independently authorized content",
            "not for acquired Agensi instructions",
        ],
    );
    contains_all(&flow(), &["Existing local files are preserved"]);
    contains_all(&text("CONTEXT.md"), &["Original Skill Package"]);
}
#[test]
fn test_native_plugin_versions_agree() {
    contains_all(
        &text("skills/agensi-operator/SKILL.md"),
        &["version: \"1.3.0\""],
    );
    for client in ["claude", "codex", "cursor"] {
        let value: Value =
            serde_json::from_str(&text(&format!(".{client}-plugin/plugin.json"))).unwrap();
        assert_eq!(value["version"], "1.3.0");
        assert_eq!(value["skills"], "./skills/");
    }
    let market: Value = serde_json::from_str(&text(".claude-plugin/marketplace.json")).unwrap();
    assert_eq!(market["plugins"][0]["version"], "1.3.0");
    let native: Value = serde_json::from_str(&text("plugin.json")).unwrap();
    assert_eq!(native["name"], "agensi-operator");
}
