//! Conservative source checks, not Rust name resolution or a security boundary.
use proc_macro2::{Span, TokenStream, TokenTree};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use syn::{
    Expr, ExprCall, ExprMethodCall, Lit,
    spanned::Spanned,
    visit::{self, Visit},
};

#[derive(Debug)]
pub struct Finding {
    pub file: PathBuf,
    pub line: usize,
    pub rule: &'static str,
    pub message: String,
    pub allowance: Option<String>,
}

pub struct Report {
    pub files: usize,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.findings.iter().all(|f| f.allowance.is_some())
    }

    pub fn print(&self) {
        for f in &self.findings {
            if let Some(reason) = &f.allowance {
                eprintln!(
                    "{}:{}: [placebo:allowed {}] {reason}",
                    f.file.display(),
                    f.line,
                    f.rule
                );
            } else {
                eprintln!(
                    "{}:{}: [placebo:{}] {}",
                    f.file.display(),
                    f.line,
                    f.rule,
                    f.message
                );
            }
        }
        let errors = self
            .findings
            .iter()
            .filter(|f| f.allowance.is_none())
            .count();
        let allowed = self.findings.len() - errors;
        eprintln!(
            "[placebo:check] {} Rust file(s), {errors} error(s), {allowed} explicit exception(s). Source checks do not replace Cargo or browser tests.",
            self.files
        );
    }
}

struct Source {
    file: PathBuf,
    text: String,
    ast: syn::File,
}

#[derive(Clone)]
struct Action {
    symbol: Option<String>,
    path: Option<String>,
}

fn last_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        Expr::Paren(p) => last_name(&p.expr),
        _ => None,
    }
}

fn string(expr: &Expr) -> Option<String> {
    if let Expr::Lit(lit) = expr
        && let Lit::Str(s) = &lit.lit
    {
        return Some(s.value());
    }
    None
}

fn test_module(module: &syn::ItemMod) -> bool {
    module.attrs.iter().any(|a| {
        a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test")
    })
}

struct Declarations {
    names: Vec<String>,
    actions: Vec<Action>,
}

impl Declarations {
    fn constructor(&self, expr: &Expr) -> Option<Action> {
        let Expr::Call(call) = expr else { return None };
        let Expr::Path(p) = call.func.as_ref() else {
            return None;
        };
        let mut parts = p.path.segments.iter().rev();
        if parts.next()?.ident != "new"
            || !self.names.contains(&parts.next()?.ident.to_string())
            || call.args.len() != 2
        {
            return None;
        }
        Some(Action {
            symbol: None,
            path: string(&call.args[1]),
        })
    }
}

impl<'ast> Visit<'ast> for Declarations {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !test_module(node) {
            visit::visit_item_mod(self, node);
        }
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        if let Some(mut action) = self.constructor(&node.expr) {
            action.symbol = Some(node.ident.to_string());
            self.actions.push(action);
        }
        visit::visit_item_const(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        if let Some(mut action) = self.constructor(&node.expr) {
            action.symbol = Some(node.ident.to_string());
            self.actions.push(action);
        }
        visit::visit_item_static(self, node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        if let Some(init) = &node.init
            && let Some(mut action) = self.constructor(&init.expr)
        {
            let pat = match &node.pat {
                syn::Pat::Type(p) => p.pat.as_ref(),
                p => p,
            };
            if let syn::Pat::Ident(p) = pat {
                action.symbol = Some(p.ident.to_string());
            }
            self.actions.push(action);
        }
        visit::visit_local(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Some(action) = self.constructor(&Expr::Call(node.clone())) {
            self.actions.push(action);
        }
        visit::visit_expr_call(self, node);
    }
}

// Follow explicit use-renames of the two public action types. This deliberately
// does not implement Rust's full import, re-export, macro, or type resolution.
fn action_aliases(tree: &syn::UseTree, names: &mut Vec<String>) {
    match tree {
        syn::UseTree::Rename(r)
            if matches!(
                r.ident.to_string().as_str(),
                "ReadAction" | "MutationAction"
            ) =>
        {
            names.push(r.rename.to_string())
        }
        syn::UseTree::Path(p) => action_aliases(&p.tree, names),
        syn::UseTree::Group(g) => {
            for t in &g.items {
                action_aliases(t, names);
            }
        }
        _ => {}
    }
}

struct Aliases(Vec<String>);
impl<'ast> Visit<'ast> for Aliases {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        action_aliases(&node.tree, &mut self.0);
    }
}

struct Inspector<'a> {
    source: &'a Source,
    declarations: &'a Declarations,
    findings: Vec<Finding>,
    excluded: Vec<(usize, usize)>,
}

impl Inspector<'_> {
    fn finding(&mut self, span: Span, rule: &'static str, message: &str) {
        self.findings.push(Finding {
            file: self.source.file.clone(),
            line: span.start().line,
            rule,
            message: message.into(),
            allowance: None,
        });
    }

    fn raw_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        for (i, token) in tokens.iter().enumerate() {
            if matches!(token, TokenTree::Ident(x) if x == "data")
                && matches!(tokens.get(i+1), Some(TokenTree::Punct(x)) if x.as_char() == '-')
                && matches!(tokens.get(i+2), Some(TokenTree::Ident(x)) if x == "placebo")
                && matches!(tokens.get(i+3), Some(TokenTree::Punct(x)) if x.as_char() == '=')
            {
                self.finding(token.span(), "raw-config", "Handwritten data-placebo configuration bypasses typed form generation. Use fields! and action.bind(...).form(fields).");
            }
            match token {
                TokenTree::Group(g) => self.raw_tokens(g.stream()),
                TokenTree::Literal(lit) => {
                    if let Ok(s) = syn::parse2::<syn::LitStr>(TokenStream::from(
                        TokenTree::Literal(lit.clone()),
                    )) {
                        self.raw_string(&s);
                    }
                }
                _ => {}
            }
        }
    }

    fn raw_string(&mut self, literal: &syn::LitStr) {
        self.excluded
            .push((literal.span().start().line, literal.span().end().line));
        let text = literal.value();
        if text.contains('<')
            && text.match_indices("data-placebo").any(|(i, _)| {
                let before = &text[..i];
                before.ends_with(char::is_whitespace)
                    && text[i + 12..].trim_start().starts_with('=')
            })
        {
            self.finding(literal.span(), "raw-config", "HTML string contains handwritten data-placebo configuration. Render the form with a typed action binding; isolate intentional custom rendering with an explicit exception.");
        }
    }

    fn action_paths(&self, expr: &Expr) -> Vec<Option<String>> {
        if let Some(action) = self.declarations.constructor(expr) {
            return vec![action.path];
        }
        let Some(name) = last_name(expr) else {
            return Vec::new();
        };
        self.declarations
            .actions
            .iter()
            .filter(|a| a.symbol.as_ref() == Some(&name))
            .map(|a| a.path.clone())
            .collect()
    }

    fn route_target(&self, expr: &Expr) -> Option<Vec<Option<String>>> {
        if let Some(path) = string(expr)
            && self
                .declarations
                .actions
                .iter()
                .any(|a| a.path.as_ref() == Some(&path))
        {
            return Some(vec![Some(path)]);
        }
        if let Expr::MethodCall(m) = expr
            && m.method == "path"
        {
            let paths = self.action_paths(&m.receiver);
            if !paths.is_empty() {
                return Some(paths);
            }
        }
        None
    }

    fn typed_adapter(&self, expr: &Expr, targets: &[Option<String>]) -> bool {
        match expr {
            Expr::Paren(p) => self.typed_adapter(&p.expr, targets),
            Expr::MethodCall(m) if m.method == "route" && m.args.len() == 1 => self
                .action_paths(&m.receiver)
                .iter()
                .any(|p| targets.contains(p)),
            Expr::MethodCall(m)
                if matches!(
                    m.method.to_string().as_str(),
                    "layer" | "route_layer" | "with_state"
                ) =>
            {
                self.typed_adapter(&m.receiver, targets)
            }
            Expr::MethodCall(m) if m.method == "merge" && m.args.len() == 1 => {
                self.typed_adapter(&m.receiver, targets) && self.typed_adapter(&m.args[0], targets)
            }
            _ => false,
        }
    }
}

impl<'ast> Visit<'ast> for Inspector<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if test_module(node) {
            self.excluded
                .push((node.span().start().line, node.span().end().line));
        } else {
            visit::visit_item_mod(self, node);
        }
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.raw_tokens(node.tokens.clone());
    }
    fn visit_lit_str(&mut self, node: &'ast syn::LitStr) {
        self.raw_string(node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "route"
            && node.args.len() == 2
            && let Some(targets) = self.route_target(&node.args[0])
            && !self.typed_adapter(&node.args[1], &targets)
        {
            self.finding(node.method.span(), "untyped-route", "A declared Placebo action is registered without a visible matching action.route(handler) adapter. Use that adapter for reads and mutations; ordinary non-Placebo Axum routes are unaffected.");
        }
        visit::visit_expr_method_call(self, node);
    }
}

fn allowances(source: &Source, inspector: &mut Inspector<'_>) {
    let lines: Vec<_> = source.text.lines().collect();
    let mut directives = BTreeMap::new();
    for (i, line) in lines.iter().enumerate() {
        if inspector
            .excluded
            .iter()
            .any(|(start, end)| *start <= i + 1 && i < *end)
        {
            continue;
        }
        let Some(body) = line.trim().strip_prefix("// placebo:allow ") else {
            continue;
        };
        let parsed = body.split_once(" -- ").filter(|(rule, reason)| {
            matches!(*rule, "raw-config" | "untyped-route") && !reason.trim().is_empty()
        });
        if let Some((rule, reason)) = parsed {
            let target = lines
                .iter()
                .enumerate()
                .skip(i + 1)
                .find(|(_, l)| !l.trim().is_empty())
                .map(|(n, _)| n + 1);
            directives.insert(
                (target.unwrap_or(0), rule.to_string()),
                (i + 1, reason.trim().to_string(), false),
            );
        } else {
            inspector.findings.push(Finding {
                file: source.file.clone(),
                line: i + 1,
                rule: "invalid-allow",
                message: "Expected // placebo:allow raw-config|untyped-route -- a nonempty reason"
                    .into(),
                allowance: None,
            });
        }
    }
    for finding in &mut inspector.findings {
        if let Some((_, reason, used)) =
            directives.get_mut(&(finding.line, finding.rule.to_string()))
        {
            finding.allowance = Some(reason.clone());
            *used = true;
        }
    }
    for ((_, rule), (line, _, used)) in directives {
        if !used {
            inspector.findings.push(Finding { file: source.file.clone(), line, rule: "unused-allow", message: format!("No {rule} finding on the next nonblank line. Remove this stale exception or place it immediately above the flagged line."), allowance: None });
        }
    }
}

fn inspect(sources: &[Source]) -> Vec<Finding> {
    let mut aliases = Aliases(vec!["ReadAction".into(), "MutationAction".into()]);
    for source in sources {
        aliases.visit_file(&source.ast);
    }
    let mut declarations = Declarations {
        names: aliases.0,
        actions: Vec::new(),
    };
    for source in sources {
        declarations.visit_file(&source.ast);
    }
    let mut result = Vec::new();
    for source in sources {
        let mut inspector = Inspector {
            source,
            declarations: &declarations,
            findings: Vec::new(),
            excluded: Vec::new(),
        };
        inspector.visit_file(&source.ast);
        allowances(source, &mut inspector);
        result.extend(inspector.findings);
    }
    result.sort_by(|a, b| (&a.file, a.line, a.rule).cmp(&(&b.file, b.line, b.rule)));
    result.dedup_by(|a, b| a.file == b.file && a.line == b.line && a.rule == b.rule);
    result
}

fn rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory).map_err(|e| format!("{}: {e}", directory.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_symlink() {
            return Err(format!(
                "{}: source symlinks are not supported by placebo check; check the source package directly.",
                path.display()
            ));
        }
        if kind.is_dir() {
            rust_files(&path, files)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

pub fn check(root: &Path, examples: bool) -> Result<Report, String> {
    if !root.join("Cargo.toml").is_file() {
        return Err("Run from a Cargo package directory, or use --path DIR.".into());
    }
    let mut files = Vec::new();
    rust_files(
        &root.join(if examples { "examples" } else { "src" }),
        &mut files,
    )?;
    files.sort();
    if files.is_empty() {
        return Err("No Rust source files found in the selected src/ or examples/ directory. Custom Cargo target paths require checking a supported source layout.".into());
    }
    let mut sources = Vec::new();
    let mut parse_errors = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        match syn::parse_file(&text) {
            Ok(ast) => sources.push(Source {
                file: file.strip_prefix(root).unwrap_or(file).to_path_buf(),
                text,
                ast,
            }),
            Err(error) => parse_errors.push(Finding {
                file: file.strip_prefix(root).unwrap_or(file).to_path_buf(),
                line: error.span().start().line,
                rule: "syntax",
                message: error.to_string(),
                allowance: None,
            }),
        }
    }
    let mut findings = inspect(&sources);
    findings.extend(parse_errors);
    Ok(Report {
        files: files.len(),
        findings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scan(text: &str) -> Vec<Finding> {
        inspect(&[Source {
            file: "src/main.rs".into(),
            text: text.into(),
            ast: syn::parse_file(text).unwrap(),
        }])
    }

    #[test]
    fn catches_manual_config_but_not_local_keys_or_comments() {
        let f = scan(
            r#"fn view() { html! { form data-placebo=(config) {} div data-placebo-local="draft" {} } }
            // form data-placebo=(comment)
            fn text() { let x = "<form data-placebo='raw'>"; }"#,
        );
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|f| f.rule == "raw-config"));
    }

    #[test]
    fn catches_read_and_mutation_bypasses_without_banning_axum() {
        let f = scan(
            r#"use placebo::MutationAction as Mutation;
            const SAVE: Mutation<Input> = Mutation::new("save", "/save");
            const SEARCH: placebo::ReadAction<Input> = placebo::ReadAction::new("search", "/search");
            fn router() { Router::new().route("/", get(home))
                .route("/save", post(save))
                .route(SEARCH.path(), get(search))
                .route("/api", post(api)); }"#,
        );
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|f| f.rule == "untyped-route"));
    }

    #[test]
    fn accepts_typed_adapters_and_layers_but_rejects_wrong_action() {
        let f = scan(
            r#"const SAVE: MutationAction<Input> = MutationAction::new("save", "/save");
            const OTHER: MutationAction<Input> = MutationAction::new("other", "/other");
            fn router() { Router::new().route(SAVE.path(), SAVE.route(save).layer(guard))
                .route("/save", OTHER.route(other)); }"#,
        );
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn allowances_are_local_reasoned_and_visible() {
        let f = scan(
            "fn view() {\n// placebo:allow raw-config -- Vendor bridge owns this form.\nhtml! { form data-placebo=(config) {} }\nhtml! { form data-placebo=(other) {} }\n}",
        );
        assert_eq!(f.len(), 2);
        assert_eq!(
            f[0].allowance.as_deref(),
            Some("Vendor bridge owns this form.")
        );
        assert!(f[1].allowance.is_none());
        let stale = scan("// placebo:allow raw-config -- Old integration.\nfn unrelated() {}");
        assert_eq!(stale[0].rule, "unused-allow");
        let invalid = scan("// placebo:allow raw-config -- \nfn unrelated() {}");
        assert_eq!(invalid[0].rule, "invalid-allow");
    }

    #[test]
    fn reads_declarations_across_files_and_handles_local_actions() {
        let sources: Vec<_> = [
            ("src/actions.rs", "pub const SAVE: MutationAction<Input> = MutationAction::new(\"save\", \"/save\");"),
            ("src/main.rs", "fn router() { let search = ReadAction::new(\"search\", \"/search\"); Router::new().route(actions::SAVE.path(), post(save)).route(search.path(), search.route(read)); }")
        ].into_iter().map(|(file, text)| Source { file: file.into(), text: text.into(), ast: syn::parse_file(text).unwrap() }).collect();
        assert_eq!(inspect(&sources).len(), 1);
    }
}
