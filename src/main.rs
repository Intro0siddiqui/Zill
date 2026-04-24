use std::io::{self, Write};
use zill::ZillSession;

fn main() {
    let mut session = ZillSession::new();
    println!("Zill REPL - type 'exit' to quit");

    loop {
        print!("zill> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).unwrap() == 0 {
            break;
        }

        let input = input.trim();
        if input == "exit" {
            break;
        }

        if input.is_empty() {
            continue;
        }

        let output = session.run(input);
        if !output.stdout.is_empty() {
            print!("{}", output.stdout);
        }
        if !output.stderr.is_empty() {
            eprintln!("{}", output.stderr);
        }
    }
}
