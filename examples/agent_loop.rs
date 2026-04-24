use zill::ZillSession;

fn main() {
    let mut session = ZillSession::new();

    // Simulate a sequence of commands an agent might run
    let commands = vec![
        "mkdir -p /src/project",
        "echo 'fn main() { println!(\"hello\"); }' > /src/project/main.rs",
        "ls -la /src/project",
        "rg println /src/project",
        "fd . /src",
    ];

    for cmd in commands {
        println!("Agent running: {}", cmd);
        let output = session.run(cmd);
        if !output.stdout.is_empty() {
            println!("STDOUT:\n{}", output.stdout);
        }
        if !output.stderr.is_empty() {
            println!("STDERR:\n{}", output.stderr);
        }
        println!("---");
    }

    // Demonstrate serialization
    println!("Serializing session...");
    let json = session.to_json().unwrap();
    println!("Serialized size: {} bytes", json.len());

    let mut session2 = ZillSession::from_json(&json).unwrap();
    println!("Resuming in new session...");
    let output = session2.run("cat /src/project/main.rs");
    println!("STDOUT:\n{}", output.stdout);
}
