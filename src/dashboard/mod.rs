//! `flux dashboard` — a self-contained static HTML report.
//!
//! The spec asks for a "developer dashboard". A *served* web dashboard is a
//! documented non-goal (it needs a server and network stack Flux avoids). The
//! honest substitute is a single self-contained HTML file — no network, no
//! external assets, inline CSS — rendered from the same intelligence the CLI
//! uses. You open it in a browser; it's a real artifact, not a fake service.

use std::path::{Path, PathBuf};

use crate::intel::Intelligence;

/// Where the dashboard is written.
pub fn path(root: &Path) -> PathBuf {
    root.join(".flux-cache")
        .join("reports")
        .join("dashboard.html")
}

/// Analyse the project and write the dashboard, returning its path.
pub fn write(root: &Path) -> std::io::Result<PathBuf> {
    let intel = crate::intel::analyze(root);
    let html = render(&intel);
    let out = path(root);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out, html)?;
    Ok(out)
}

/// Render the dashboard HTML for an analysis. Pure function → easy to test.
pub fn render(intel: &Intelligence) -> String {
    let health_color = match intel.health.score {
        90..=100 => "#2ecc71",
        75..=89 => "#27ae60",
        50..=74 => "#f39c12",
        _ => "#e74c3c",
    };

    let languages: String = intel
        .languages
        .iter()
        .map(|(lang, n)| format!("<li><span>{}</span><b>{n}</b></li>", esc(lang)))
        .collect();

    let components: String = if intel.components.is_empty() {
        "<li class=\"muted\">no components detected</li>".to_string()
    } else {
        intel
            .components
            .iter()
            .map(|c| {
                let deps = if c.depends_on.is_empty() {
                    String::new()
                } else {
                    format!(
                        " <span class=\"dep\">→ {}</span>",
                        esc(&c.depends_on.join(", "))
                    )
                };
                format!(
                    "<li><span>{}{deps}</span><b>{} files</b></li>",
                    esc(&c.name),
                    c.files
                )
            })
            .collect()
    };

    let signals: String = intel
        .health
        .signals
        .iter()
        .map(|s| {
            let (cls, mark) = if s.ok { ("ok", "✓") } else { ("gap", "✗") };
            format!(
                "<li class=\"{cls}\"><span>{mark} {}</span><small>{}</small></li>",
                esc(&s.name),
                esc(&s.detail)
            )
        })
        .collect();

    let recommendations: String = {
        let gaps = intel.health.gaps();
        if gaps.is_empty() {
            "<li class=\"ok\">✓ No outstanding recommendations</li>".to_string()
        } else {
            gaps.iter()
                .take(5)
                .map(|g| {
                    format!(
                        "<li>⚠ <b>+{}</b> {} — {}</li>",
                        g.weight,
                        esc(&g.name),
                        esc(&g.detail)
                    )
                })
                .collect()
        }
    };

    let git = if intel.git.is_repo {
        format!(
            "{} commits · {} contributor(s){}",
            intel.git.commits,
            intel.git.contributors,
            intel
                .git
                .last_commit
                .as_ref()
                .map(|d| format!(" · last commit {}", esc(d)))
                .unwrap_or_default()
        )
    } else {
        "not a git repository".to_string()
    };

    let lang = intel
        .primary_language
        .as_ref()
        .map(|l| crate::intel::language_display(l))
        .unwrap_or_else(|| "—".into());

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Flux — {project}</title>
<style>
:root {{ color-scheme: light dark; }}
* {{ box-sizing: border-box; }}
body {{ font: 15px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
       margin: 0; background: #0e1116; color: #e6edf3; }}
header {{ padding: 32px; background: linear-gradient(135deg,#1b2230,#0e1116); border-bottom:1px solid #222; }}
h1 {{ margin: 0; font-size: 22px; }}
h1 span {{ color: #58a6ff; }}
.sub {{ color: #8b949e; margin-top: 4px; }}
main {{ max-width: 1000px; margin: 0 auto; padding: 24px; display: grid; gap: 20px;
        grid-template-columns: repeat(auto-fit,minmax(280px,1fr)); }}
.card {{ background:#161b22; border:1px solid #21262d; border-radius:10px; padding:18px; }}
.card h2 {{ margin:0 0 12px; font-size:13px; text-transform:uppercase; letter-spacing:.06em; color:#8b949e; }}
.score {{ display:flex; align-items:center; gap:16px; }}
.ring {{ width:84px; height:84px; border-radius:50%; display:grid; place-items:center;
         font-size:24px; font-weight:700; color:#fff; background:{health_color}; }}
ul {{ list-style:none; margin:0; padding:0; }}
li {{ display:flex; justify-content:space-between; gap:10px; padding:5px 0; border-bottom:1px solid #21262d; }}
li:last-child {{ border-bottom:none; }}
li.ok small {{ color:#3fb950; }}
li.gap small {{ color:#8b949e; }}
li.ok span {{ color:#3fb950; }}
li.gap span {{ color:#f85149; }}
.dep {{ color:#8b949e; font-size:12px; }}
.muted {{ color:#8b949e; }}
small {{ color:#8b949e; }}
footer {{ text-align:center; color:#8b949e; padding:20px; font-size:12px; }}
b {{ color:#e6edf3; }}
</style>
</head>
<body>
<header>
<h1><span>Flux</span> · {project}</h1>
<div class="sub">{lang} · {files} source files · {git}</div>
</header>
<main>
  <section class="card">
    <h2>Project health</h2>
    <div class="score">
      <div class="ring">{score}</div>
      <div><b>{grade}</b><div class="sub">{killer}</div></div>
    </div>
  </section>
  <section class="card">
    <h2>Recommendations</h2>
    <ul>{recommendations}</ul>
  </section>
  <section class="card">
    <h2>Languages</h2>
    <ul>{languages}</ul>
  </section>
  <section class="card">
    <h2>Architecture</h2>
    <ul>{components}</ul>
  </section>
  <section class="card">
    <h2>Dependencies</h2>
    <ul><li><span>Declared{dep_source}</span><b>{dep_total}</b></li>
        <li><span>Lockfile</span><b>{locked}</b></li></ul>
  </section>
  <section class="card">
    <h2>Health signals</h2>
    <ul>{signals}</ul>
  </section>
</main>
<footer>Generated locally by <b>flux dashboard</b> — no data leaves this machine.</footer>
</body>
</html>
"#,
        project = esc(&intel.project),
        lang = esc(&lang),
        files = intel.file_count,
        git = esc(&git),
        score = intel.health.score,
        grade = esc(intel.health.grade()),
        killer = if intel.has_killer {
            "Secured by Killer"
        } else {
            "Killer not detected"
        },
        recommendations = recommendations,
        languages = languages,
        components = components,
        dep_source = intel
            .dependencies
            .source
            .as_ref()
            .map(|s| format!(" in {}", esc(s)))
            .unwrap_or_default(),
        dep_total = intel.dependencies.total,
        locked = if intel.dependencies.locked {
            "yes"
        } else {
            "no"
        },
        signals = signals,
        health_color = health_color,
    )
}

/// Escape text for safe embedding in HTML.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_prevents_injection() {
        assert_eq!(esc("<b>&\"x\""), "&lt;b&gt;&amp;&quot;x&quot;");
    }

    #[test]
    fn render_includes_project_and_score() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-dash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"dashdemo\"\n").unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();

        let intel = crate::intel::analyze(&dir);
        let html = render(&intel);
        assert!(html.contains("dashdemo"));
        assert!(html.contains("Project health"));
        assert!(html.starts_with("<!doctype html>"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
