use zill::ZillSession;
use serde_json::json;

fn main() {
    let mut session = ZillSession::new();

    // Create synthetic issues.json
    let mut issues = Vec::new();
    for i in 1..=200 {
        let labels = if i % 10 == 0 { vec!["security"] }
                     else if i % 5 == 0 { vec!["bug"] }
                     else { vec!["enhancement"] };
        let body = if i % 7 == 0 { "TODO: fix this" } else { "Issue description" };

        issues.push(json!({
            "number": i,
            "title": format!("Issue #{}", i),
            "body": body,
            "state": "open",
            "labels": labels,
            "created_at": "2026-04-23T20:00:00Z"
        }));
    }

    let issues_json = serde_json::to_string_pretty(&issues).unwrap();
    session.run(&format!("echo '{}' > issues.json", issues_json.replace("'", "'\\''")));

    println!("Evaluating queries...");

    // Query 1: rg -c security issues.json
    let out1 = session.run("rg -c security issues.json");
    println!("Query: rg -c security issues.json");
    println!("Output: {}", out1.stdout);

    // Query 2: fd -e json
    let out2 = session.run("fd -e json");
    println!("Query: fd -e json");
    println!("Output: {}", out2.stdout);

    // Query 3: rg "bug" | rg "fix" (simulated with two runs)
    println!("Query: rg \"bug\" (simulated pipe to rg \"fix\")");
    let out3a = session.run("rg bug issues.json");
    // Simulate pipe by writing output to temporary file then grepping that
    session.run(&format!("echo '{}' > tmp.txt", out3a.stdout.replace("'", "'\\''")));
    let out3b = session.run("rg fix tmp.txt");
    println!("Output: {}", out3b.stdout);
}
