use zill::{ZillSession, ZillLimits};
use std::path::PathBuf;

#[test]
fn stress_test_massive_codebase() {
    let mut session = ZillSession::with_limits(ZillLimits {
        max_nodes: 11000,
        ..Default::default()
    });

    for i in 0..10000 {
        let path = format!("/src/file_{}.rs", i);
        let content = if i % 100 == 0 { "TODO: fix this" } else { "println!(\"hello\");" };
        session.vfs.create_file(&PathBuf::from(path), content.as_bytes().to_vec()).unwrap();
    }

    let out = session.run("rg -n TODO /src");
    assert_eq!(out.stdout.lines().count(), 100);
}

#[test]
fn stress_test_output_flood() {
    let mut session = ZillSession::new();
    let mut big_content = String::new();
    for _ in 0..1000 {
        big_content.push_str("match me\n");
    }
    session.vfs.create_file(&PathBuf::from("/big.txt"), big_content.as_bytes().to_vec()).unwrap();

    let out = session.run("rg match /big.txt");
    assert!(out.stdout.len() <= session.limits.max_output_size + 1024); // allow some overflow for the last line

    let out_count = session.run("rg -c match /big.txt");
    assert!(out_count.stdout.contains("1000"));
}

#[test]
fn stress_test_path_canonicalization_torture() {
    let mut session = ZillSession::new();
    session.run("mkdir -p /a/b/c/d");
    session.run("cd /a/b/c/d");
    let out = session.run("pwd");
    assert_eq!(out.stdout, "/a/b/c/d\n");

    session.run("cd ../../../..");
    let out = session.run("pwd");
    assert_eq!(out.stdout, "/\n");

    session.run("mkdir -p /c/d"); // Ensure target exists for cd
    session.run("cd /a/b/../../c/./d");
    let out = session.run("pwd");
    assert_eq!(out.stdout, "/c/d\n");
}

#[test]
fn stress_test_large_file_boundary() {
    let mut session = ZillSession::new();
    let big_data = vec![0u8; session.limits.max_file_size + 1];
    let res = session.vfs.create_file(&PathBuf::from("/too_big.bin"), big_data);
    assert!(res.is_err());
}

#[test]
fn stress_test_serialization_roundtrip() {
    let mut session = ZillSession::new();
    session.run("mkdir /work");
    session.run("echo 'hello' > /work/a.txt");

    let bincode_data = bincode::serialize(&session).unwrap();
    let mut session2: ZillSession = bincode::deserialize(&bincode_data).unwrap();

    let out = session2.run("cat /work/a.txt");
    assert_eq!(out.stdout, "hello\n");
}

#[test]
fn stress_test_multi_session_determinism() {
    let mut session1 = ZillSession::new();
    let mut session2 = ZillSession::new();

    let cmd = "echo 'hello' > file.txt";
    session1.run(cmd);
    session2.run(cmd);

    assert_eq!(session1.run("cat file.txt").stdout, session2.run("cat file.txt").stdout);
}
