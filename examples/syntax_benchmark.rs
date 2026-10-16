//! Synthetic-only timings; no user note contents are logged.
use opalglass::prepared::Prepared;
use std::time::Instant;
fn main() {
    let code = "const café = 42; // comment\nconsole.log(\"你好\");\n";
    for (name, source) in [
        ("cold-small", format!("```javascript\n{code}```\n")),
        ("warm-small", format!("```javascript\n{code}```\n")),
        (
            "long-multiline",
            format!("```javascript\n{}```\n", code.repeat(5000)),
        ),
        (
            "long-cached",
            format!("```javascript\n{}```\n", code.repeat(5000)),
        ),
        (
            "minified",
            format!("```javascript\n{}\n```\n", "const a=42;".repeat(100_000)),
        ),
    ] {
        let start = Instant::now();
        let opening = Prepared::opening(&source);
        let opening_ms = start.elapsed().as_secs_f64() * 1000.;
        let start = Instant::now();
        let structure =
            Prepared::undecorated_cancellable(&source, &std::sync::atomic::AtomicBool::new(false))
                .unwrap();
        let structure_ms = start.elapsed().as_secs_f64() * 1000.;
        let start = Instant::now();
        let complete = Prepared::new(&source);
        println!(
            "{}",
            serde_json::json!({"case":name,"bytes":source.len(),"opening_ms":opening_ms,"structure_ms":structure_ms,"complete_ms":start.elapsed().as_secs_f64()*1000.,"opening_rows":opening.rows.len(),"rows":complete.rows.len(),"structural_rows":structure.rows.len(),"source_tokens":complete.source_syntax.len()})
        );
    }
}
