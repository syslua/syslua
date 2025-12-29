//! Placeholder parsing and substitution for deferred value resolution.
//!
//! Placeholders allow builds and binds to reference values that aren't known
//! until execution time. This module handles parsing placeholder strings and
//! substituting resolved values.
//!
//! # Placeholder Formats
//!
//! - `$${action:N}` - stdout of action at index N within the same spec
//! - `$${build:<hash>:<output>}` - output from a realized build
//! - `$${bind:<hash>:<output>}` - output from an applied bind
//!
//! # Shell Variables
//!
//! Single `$` characters pass through unchanged, so shell variables like
//! `$HOME` and `$PATH` work naturally without any escaping.
//!
//! # Escaping
//!
//! Use `$$$` before `{` to produce a literal `$${` sequence. This is only
//! needed in the rare case where you want literal `$${` in output.
//!
//! # Example
//!
//! ```
//! use syslua_lib::placeholder::{parse, Segment, Placeholder};
//!
//! let segments = parse("$${action:0}/bin:$HOME").unwrap();
//! assert_eq!(segments, vec![
//!     Segment::Placeholder(Placeholder::Action(0)),
//!     Segment::Literal("/bin:$HOME".to_string()),
//! ]);
//! ```

use thiserror::Error;

/// A parsed placeholder reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placeholder {
  /// `$${action:N}` - stdout of action at index N
  Action(usize),

  /// `$${build:<hash>:<output>}` - output from realized build
  Build { hash: String, output: String },

  /// `$${bind:<hash>:<output>}` - output from applied bind
  Bind { hash: String, output: String },

  /// `$${out}` - the current build/bind's output directory
  Out,
}

/// A segment of parsed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
  /// Literal text (no placeholders)
  Literal(String),

  /// A placeholder to be resolved
  Placeholder(Placeholder),
}

/// Errors that can occur during placeholder parsing or resolution.
#[derive(Debug, Clone, PartialEq, Eq, Error, serde::Serialize, serde::Deserialize)]
pub enum PlaceholderError {
  #[error("unclosed placeholder at position {0}")]
  Unclosed(usize),

  #[error("unknown placeholder type: {0}")]
  UnknownType(String),

  #[error("invalid action index: {0}")]
  InvalidActionIndex(String),

  #[error("malformed placeholder: {0}")]
  Malformed(String),

  #[error("unresolved action: index {0}")]
  UnresolvedAction(usize),

  #[error("unresolved build: {hash} output '{output}'")]
  UnresolvedBuild { hash: String, output: String },

  #[error("unresolved bind: {hash} output '{output}'")]
  UnresolvedBind { hash: String, output: String },
}

/// Trait for resolving placeholder values during execution.
pub trait Resolver {
  /// Resolve an action output by index.
  fn resolve_action(&self, index: usize) -> Result<&str, PlaceholderError>;

  /// Resolve a build output by hash and output name.
  fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError>;

  /// Resolve a bind output by hash and output name.
  fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError>;

  /// Resolve the output directory for the current build/bind.
  fn resolve_out(&self) -> Result<&str, PlaceholderError>;
}

/// Parse a string containing placeholders into segments.
///
/// # Placeholder Formats
///
/// - `$${action:N}` - reference action stdout at index N
/// - `$${build:HASH:OUTPUT}` - reference build output
/// - `$${bind:HASH:OUTPUT}` - reference bind output
/// - `$${out}` - reference the current build/bind's output directory
///
/// # Escaping
///
/// Use `$$$` before `{` to produce a literal `$$` followed by `{`.
/// Single `$` characters pass through unchanged, so shell variables
/// like `$HOME` work naturally without escaping.
///
/// # Errors
///
/// Returns an error if a placeholder is malformed (unclosed, unknown type, etc.)
pub fn parse(input: &str) -> Result<Vec<Segment>, PlaceholderError> {
  let mut segments = Vec::new();
  let mut literal = String::new();
  let mut chars = input.char_indices().peekable();

  while let Some((pos, ch)) = chars.next() {
    if ch == '$' {
      // Check what follows the first $
      match chars.peek() {
        Some((_, '$')) => {
          // We have "$$", check what follows
          chars.next(); // consume the second $

          match chars.peek() {
            Some((_, '$')) => {
              // We have "$$$", check if next is "{"
              // This is the escape sequence: $$$ + { -> $$ + {
              chars.next(); // consume the third $

              match chars.peek() {
                Some((_, '{')) => {
                  // Escaped: $$${ -> $${ (literal)
                  literal.push_str("$${");
                  chars.next(); // consume the {
                }
                _ => {
                  // Just "$$$" followed by something else, output as literal
                  literal.push_str("$$$");
                }
              }
            }
            Some((_, '{')) => {
              // We have "$${" - this is a placeholder
              chars.next(); // consume the {

              // Flush accumulated literal
              if !literal.is_empty() {
                segments.push(Segment::Literal(std::mem::take(&mut literal)));
              }

              // Find the closing brace
              let mut placeholder_content = String::new();
              let mut found_close = false;

              for (_, c) in chars.by_ref() {
                if c == '}' {
                  found_close = true;
                  break;
                }
                placeholder_content.push(c);
              }

              if !found_close {
                return Err(PlaceholderError::Unclosed(pos));
              }

              // Parse the placeholder content
              let placeholder = parse_placeholder_content(&placeholder_content)?;
              segments.push(Segment::Placeholder(placeholder));
            }
            _ => {
              // Just "$$" followed by something other than $ or {, output as literal
              literal.push_str("$$");
            }
          }
        }
        _ => {
          // Just a lone $, treat as literal (shell variables like $HOME pass through)
          literal.push('$');
        }
      }
    } else {
      literal.push(ch);
    }
  }

  // Flush any remaining literal
  if !literal.is_empty() {
    segments.push(Segment::Literal(literal));
  }

  Ok(segments)
}

/// Parse the content inside a placeholder (everything between ${ and }).
fn parse_placeholder_content(content: &str) -> Result<Placeholder, PlaceholderError> {
  // Handle special case: "out" has no colon
  if content == "out" {
    return Ok(Placeholder::Out);
  }

  // Split by first colon to get the type
  let (kind, rest) = content
    .split_once(':')
    .ok_or_else(|| PlaceholderError::Malformed(format!("missing colon in '{content}'")))?;

  match kind {
    "action" => {
      let index = rest
        .parse::<usize>()
        .map_err(|_| PlaceholderError::InvalidActionIndex(rest.to_string()))?;
      Ok(Placeholder::Action(index))
    }
    "build" => {
      let (hash, output) = rest
        .split_once(':')
        .ok_or_else(|| PlaceholderError::Malformed(format!("build placeholder missing output: '{content}'")))?;
      Ok(Placeholder::Build {
        hash: hash.to_string(),
        output: output.to_string(),
      })
    }
    "bind" => {
      let (hash, output) = rest
        .split_once(':')
        .ok_or_else(|| PlaceholderError::Malformed(format!("bind placeholder missing output: '{content}'")))?;
      Ok(Placeholder::Bind {
        hash: hash.to_string(),
        output: output.to_string(),
      })
    }
    _ => Err(PlaceholderError::UnknownType(kind.to_string())),
  }
}

/// Substitute all placeholders in a string using the provided resolver.
///
/// This is a convenience function that parses and substitutes in one step.
///
/// # Errors
///
/// Returns an error if parsing fails or if any placeholder cannot be resolved.
pub fn substitute(input: &str, resolver: &impl Resolver) -> Result<String, PlaceholderError> {
  let segments = parse(input)?;
  substitute_segments(&segments, resolver)
}

/// Substitute placeholders in pre-parsed segments.
///
/// Use this when you've already parsed the string and want to substitute
/// multiple times with different resolvers.
pub fn substitute_segments(segments: &[Segment], resolver: &impl Resolver) -> Result<String, PlaceholderError> {
  let mut result = String::new();

  for segment in segments {
    match segment {
      Segment::Literal(s) => result.push_str(s),
      Segment::Placeholder(p) => {
        let value = match p {
          Placeholder::Action(index) => resolver.resolve_action(*index)?,
          Placeholder::Build { hash, output } => resolver.resolve_build(hash, output)?,
          Placeholder::Bind { hash, output } => resolver.resolve_bind(hash, output)?,
          Placeholder::Out => resolver.resolve_out()?,
        };
        result.push_str(value);
      }
    }
  }

  Ok(result)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashMap;

  // ==========================================================================
  // Test Resolver
  // ==========================================================================

  struct TestResolver {
    actions: Vec<String>,
    builds: HashMap<(String, String), String>,
    binds: HashMap<(String, String), String>,
    out_dir: Option<String>,
  }

  impl TestResolver {
    fn new() -> Self {
      Self {
        actions: Vec::new(),
        builds: HashMap::new(),
        binds: HashMap::new(),
        out_dir: None,
      }
    }

    fn with_action(mut self, output: &str) -> Self {
      self.actions.push(output.to_string());
      self
    }

    fn with_build(mut self, hash: &str, output_name: &str, path: &str) -> Self {
      self
        .builds
        .insert((hash.to_string(), output_name.to_string()), path.to_string());
      self
    }

    fn with_bind(mut self, hash: &str, output_name: &str, path: &str) -> Self {
      self
        .binds
        .insert((hash.to_string(), output_name.to_string()), path.to_string());
      self
    }

    fn with_out(mut self, out_dir: &str) -> Self {
      self.out_dir = Some(out_dir.to_string());
      self
    }
  }

  impl Resolver for TestResolver {
    fn resolve_action(&self, index: usize) -> Result<&str, PlaceholderError> {
      self
        .actions
        .get(index)
        .map(|s| s.as_str())
        .ok_or(PlaceholderError::UnresolvedAction(index))
    }

    fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
      self
        .builds
        .get(&(hash.to_string(), output.to_string()))
        .map(|s| s.as_str())
        .ok_or_else(|| PlaceholderError::UnresolvedBuild {
          hash: hash.to_string(),
          output: output.to_string(),
        })
    }

    fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
      self
        .binds
        .get(&(hash.to_string(), output.to_string()))
        .map(|s| s.as_str())
        .ok_or_else(|| PlaceholderError::UnresolvedBind {
          hash: hash.to_string(),
          output: output.to_string(),
        })
    }

    fn resolve_out(&self) -> Result<&str, PlaceholderError> {
      self
        .out_dir
        .as_deref()
        .ok_or(PlaceholderError::Malformed("out directory not set".to_string()))
    }
  }

  // ==========================================================================
  // Realistic Scenario Tests
  // ==========================================================================

  #[test]
  fn build_script_tar_extraction() {
    // Simulates: fetch a tarball, then extract it
    // ctx.fetch_url(...) returns $${action:0}
    // ctx.cmd("tar xf $${action:0} -C /build") uses that output
    let resolver = TestResolver::new().with_action("/tmp/ripgrep-14.1.0.tar.gz");

    let cmd = "tar xf $${action:0} -C /build && cd /build && make install";
    let result = substitute(cmd, &resolver).unwrap();

    assert_eq!(
      result,
      "tar xf /tmp/ripgrep-14.1.0.tar.gz -C /build && cd /build && make install"
    );
  }

  #[test]
  fn path_construction_multiple_builds() {
    // Simulates: constructing PATH from multiple build outputs
    // PATH=$${build:ripgrep:out}/bin:$${build:fd:out}/bin:$PATH
    let resolver = TestResolver::new()
      .with_build("a1b2c3", "out", "/store/obj/ripgrep-14.1.0-a1b2c3")
      .with_build("d4e5f6", "out", "/store/obj/fd-9.0.0-d4e5f6");

    let path_cmd = "export PATH=$${build:a1b2c3:out}/bin:$${build:d4e5f6:out}/bin:$PATH";
    let result = substitute(path_cmd, &resolver).unwrap();

    assert_eq!(
      result,
      "export PATH=/store/obj/ripgrep-14.1.0-a1b2c3/bin:/store/obj/fd-9.0.0-d4e5f6/bin:$PATH"
    );
  }

  #[test]
  fn symlink_to_config_directory() {
    // Simulates: symlinking a config file from store to user's home
    // ln -sf $${build:nvim-config:out}/init.lua ~/.config/nvim/init.lua
    let resolver = TestResolver::new().with_build("cfg123", "out", "/store/obj/nvim-config-1.0.0-cfg123");

    let cmd = "ln -sf $${build:cfg123:out}/init.lua $HOME/.config/nvim/init.lua";
    let result = substitute(cmd, &resolver).unwrap();

    assert_eq!(
      result,
      "ln -sf /store/obj/nvim-config-1.0.0-cfg123/init.lua $HOME/.config/nvim/init.lua"
    );
  }

  #[test]
  fn docker_container_from_action_output() {
    // Simulates: start a container, capture its ID, then use it later
    // container_id=$(docker run -d postgres) -> $${action:0}
    // docker exec $${action:0} psql ...
    let resolver = TestResolver::new().with_action("abc123def456");

    let cmd = "docker exec $${action:0} psql -U postgres -c 'SELECT 1'";
    let result = substitute(cmd, &resolver).unwrap();

    assert_eq!(result, "docker exec abc123def456 psql -U postgres -c 'SELECT 1'");
  }

  #[test]
  fn shell_script_with_variables() {
    // Simulates: a shell script that uses both placeholders and shell variables
    // The $1, $?, and $HOME should be preserved as shell variables (no escaping needed)
    let resolver = TestResolver::new().with_build("app123", "out", "/store/obj/myapp-1.0.0-app123");

    let script = r#"#!/bin/bash
if [ -z "$1" ]; then
  echo "Usage: $0 <arg>"
  exit 1
fi
$${build:app123:out}/bin/myapp "$1"
exit $?"#;

    let result = substitute(script, &resolver).unwrap();

    assert_eq!(
      result,
      r#"#!/bin/bash
if [ -z "$1" ]; then
  echo "Usage: $0 <arg>"
  exit 1
fi
/store/obj/myapp-1.0.0-app123/bin/myapp "$1"
exit $?"#
    );
  }

  #[test]
  fn chained_build_actions() {
    // Simulates: download -> extract -> configure -> build
    // Each action references the previous action's output
    let resolver = TestResolver::new()
      .with_action("/tmp/source.tar.gz") // action:0 - fetch result
      .with_action("/build/source") // action:1 - extract result
      .with_action("/build/source/Makefile"); // action:2 - configure result

    let make_cmd = "make -C $${action:1} -f $${action:2}";
    let result = substitute(make_cmd, &resolver).unwrap();

    assert_eq!(result, "make -C /build/source -f /build/source/Makefile");
  }

  #[test]
  fn bind_references_build_and_creates_link() {
    // Simulates: a bind that creates a symlink from a build output
    // and stores the created path as its own output
    let resolver = TestResolver::new()
      .with_build("rg123", "out", "/store/obj/ripgrep-14.1.0-rg123")
      .with_bind("bind456", "link", "/usr/local/bin/rg");

    // The bind's apply command
    let apply_cmd = "ln -sf $${build:rg123:out}/bin/rg /usr/local/bin/rg";
    let apply_result = substitute(apply_cmd, &resolver).unwrap();
    assert_eq!(
      apply_result,
      "ln -sf /store/obj/ripgrep-14.1.0-rg123/bin/rg /usr/local/bin/rg"
    );

    // The bind's destroy command (uses bind's own output)
    let destroy_cmd = "rm -f $${bind:bind456:link}";
    let destroy_result = substitute(destroy_cmd, &resolver).unwrap();
    assert_eq!(destroy_result, "rm -f /usr/local/bin/rg");
  }

  #[test]
  fn env_file_generation() {
    // Simulates: generating an env.sh file with multiple build paths
    let resolver = TestResolver::new()
      .with_build("go123", "out", "/store/obj/go-1.21.0-go123")
      .with_build("rust456", "out", "/store/obj/rust-1.75.0-rust456");

    let env_content = r#"export GOROOT=$${build:go123:out}
export CARGO_HOME=$${build:rust456:out}
export PATH=$${build:go123:out}/bin:$${build:rust456:out}/bin:$PATH"#;

    let result = substitute(env_content, &resolver).unwrap();

    assert_eq!(
      result,
      r#"export GOROOT=/store/obj/go-1.21.0-go123
export CARGO_HOME=/store/obj/rust-1.75.0-rust456
export PATH=/store/obj/go-1.21.0-go123/bin:/store/obj/rust-1.75.0-rust456/bin:$PATH"#
    );
  }

  // ==========================================================================
  // Error Cases
  // ==========================================================================

  #[test]
  fn error_unclosed_placeholder() {
    let result = parse("tar xf $${action:0");
    assert!(matches!(result, Err(PlaceholderError::Unclosed(7))));
  }

  #[test]
  fn error_unknown_placeholder_type() {
    let result = parse("$${unknown:foo}");
    assert!(matches!(result, Err(PlaceholderError::UnknownType(ref s)) if s == "unknown"));
  }

  #[test]
  fn error_invalid_action_index() {
    let result = parse("$${action:foo}");
    assert!(matches!(result, Err(PlaceholderError::InvalidActionIndex(ref s)) if s == "foo"));
  }

  #[test]
  fn error_malformed_missing_colon() {
    let result = parse("$${action}");
    assert!(matches!(result, Err(PlaceholderError::Malformed(_))));
  }

  #[test]
  fn error_build_missing_output_name() {
    let result = parse("$${build:abc123}");
    assert!(matches!(result, Err(PlaceholderError::Malformed(_))));
  }

  #[test]
  fn error_unresolved_action() {
    let resolver = TestResolver::new();
    let result = substitute("$${action:5}", &resolver);
    assert!(matches!(result, Err(PlaceholderError::UnresolvedAction(5))));
  }

  #[test]
  fn error_unresolved_build() {
    let resolver = TestResolver::new();
    let result = substitute("$${build:nonexistent:out}", &resolver);
    assert!(
      matches!(result, Err(PlaceholderError::UnresolvedBuild { ref hash, ref output })
        if hash == "nonexistent" && output == "out")
    );
  }

  #[test]
  fn error_unresolved_bind() {
    let resolver = TestResolver::new();
    let result = substitute("$${bind:nonexistent:link}", &resolver);
    assert!(
      matches!(result, Err(PlaceholderError::UnresolvedBind { ref hash, ref output })
        if hash == "nonexistent" && output == "link")
    );
  }

  // ==========================================================================
  // Edge Cases
  // ==========================================================================

  #[test]
  fn lone_dollar_preserved() {
    // $5 or $ at end of string should not be treated as placeholder
    let resolver = TestResolver::new();
    let result = substitute("costs $5 or more$", &resolver).unwrap();
    assert_eq!(result, "costs $5 or more$");
  }

  #[test]
  fn shell_variables_pass_through() {
    // Shell variables like $HOME, $PATH, $1 should pass through unchanged
    let resolver = TestResolver::new();
    let result = substitute("echo $HOME $PATH $1 $?", &resolver).unwrap();
    assert_eq!(result, "echo $HOME $PATH $1 $?");
  }

  #[test]
  fn double_dollar_without_brace_preserved() {
    // $$ without { should pass through as literal $$
    let resolver = TestResolver::new();
    let result = substitute("echo $$variable", &resolver).unwrap();
    assert_eq!(result, "echo $$variable");
  }

  #[test]
  fn escape_placeholder_syntax() {
    // $$${...} should produce literal $${...} (escape mechanism)
    let resolver = TestResolver::new();
    let result = substitute("echo $$${action:0}", &resolver).unwrap();
    assert_eq!(result, "echo $${action:0}");
  }

  #[test]
  fn empty_input() {
    let segments = parse("").unwrap();
    assert!(segments.is_empty());
  }

  #[test]
  fn adjacent_placeholders_no_separator() {
    let resolver = TestResolver::new().with_action("foo").with_action("bar");

    let result = substitute("$${action:0}$${action:1}", &resolver).unwrap();
    assert_eq!(result, "foobar");
  }

  // ==========================================================================
  // $${out} Placeholder Tests
  // ==========================================================================

  #[test]
  fn parse_out_placeholder() {
    let segments = parse("$${out}").unwrap();
    assert_eq!(segments, vec![Segment::Placeholder(Placeholder::Out)]);
  }

  #[test]
  fn parse_out_placeholder_in_path() {
    let segments = parse("$${out}/bin").unwrap();
    assert_eq!(
      segments,
      vec![
        Segment::Placeholder(Placeholder::Out),
        Segment::Literal("/bin".to_string()),
      ]
    );
  }

  #[test]
  fn substitute_out_placeholder() {
    let resolver = TestResolver::new().with_out("/store/obj/myapp-1.0.0-abc123");
    let result = substitute("mkdir -p $${out}/bin", &resolver).unwrap();
    assert_eq!(result, "mkdir -p /store/obj/myapp-1.0.0-abc123/bin");
  }

  #[test]
  fn substitute_out_with_other_placeholders() {
    let resolver = TestResolver::new()
      .with_out("/store/obj/myapp-1.0.0-abc123")
      .with_action("/tmp/source.tar.gz");

    let cmd = "tar xf $${action:0} -C $${out}";
    let result = substitute(cmd, &resolver).unwrap();
    assert_eq!(result, "tar xf /tmp/source.tar.gz -C /store/obj/myapp-1.0.0-abc123");
  }

  #[test]
  fn substitute_out_with_shell_variables() {
    let resolver = TestResolver::new().with_out("/store/bind/xyz789");
    let cmd = "ln -sf $HOME/.config/app $${out}/config";
    let result = substitute(cmd, &resolver).unwrap();
    assert_eq!(result, "ln -sf $HOME/.config/app /store/bind/xyz789/config");
  }

  #[test]
  fn error_unresolved_out() {
    let resolver = TestResolver::new(); // no out_dir set
    let result = substitute("$${out}/bin", &resolver);
    assert!(matches!(result, Err(PlaceholderError::Malformed(_))));
  }
}
