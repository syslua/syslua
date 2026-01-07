use mlua::prelude::*;

use super::common::create_test_runtime;

#[test]
fn wrap_and_unwrap() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local wrapped = priority.wrap(42, 500)
        assert(priority.is_priority(wrapped), 'wrapped should be priority')
        assert(priority.unwrap(wrapped) == 42, 'unwrap should return raw value')
        assert(priority.get_priority(wrapped) == 500, 'get_priority should return 500')
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn helpers_have_correct_priorities() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        assert(priority.get_priority(priority.force('x')) == 50, 'force should be 50')
        assert(priority.get_priority(priority.before('x')) == 500, 'before should be 500')
        assert(priority.get_priority(priority.default('x')) == 1000, 'default should be 1000')
        assert(priority.get_priority(priority.after('x')) == 1500, 'after should be 1500')
        assert(priority.get_priority(priority.order(750, 'x')) == 750, 'order(750) should be 750')
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn plain_values_have_default_priority() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        assert(not priority.is_priority(42), 'plain number should not be priority')
        assert(priority.unwrap(42) == 42, 'unwrap plain should passthrough')
        assert(priority.get_priority(42) == 1000, 'plain should have default priority')
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn order_requires_number() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  let result = lua
    .load(
      r#"
        local priority = require('syslua.priority')
        priority.order("bad", "value")
      "#,
    )
    .exec();

  assert!(result.is_err());
  let err = result.unwrap_err().to_string();
  assert!(err.contains("must be a number"), "error should mention number: {}", err);

  Ok(())
}

#[test]
fn mergeable_configuration() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local m = priority.mergeable({ separator = ':' })
        assert(priority.is_mergeable(m), 'should be mergeable')
        assert(m.separator == ':', 'separator should be :')

        local m2 = priority.mergeable()
        assert(priority.is_mergeable(m2), 'should be mergeable without opts')
        assert(m2.separator == nil, 'separator should be nil')
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn merge_lower_priority_wins() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local base = { port = priority.default(8080) }
        local override = { port = priority.before(9090) }
        local result = priority.merge(base, override)
        assert(result.port == 9090, 'before(9090) should beat default(8080)')
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn merge_force_wins() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local base = { port = priority.before(8080) }
        local override = { port = priority.force(443) }
        local result = priority.merge(base, override)
        assert(result.port == 443, 'force(443) should beat before(8080)')
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn merge_same_priority_same_value_ok() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local base = { port = priority.default(8080) }
        local override = { port = priority.default(8080) }
        local result = priority.merge(base, override)
        assert(result.port == 8080, 'same value same priority should work')
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn merge_same_priority_different_value_conflicts() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  let result = lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local base = { port = priority.default(8080) }
        local override = { port = priority.default(9090) }
        priority.merge(base, override)
      "#,
    )
    .exec();

  assert!(result.is_err());
  let err = result.unwrap_err().to_string();
  assert!(
    err.contains("Priority conflict"),
    "error should mention conflict: {}",
    err
  );
  assert!(err.contains("port"), "error should mention key: {}", err);

  Ok(())
}

#[test]
fn mergeable_string_combines_with_separator() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local base = { paths = priority.mergeable({ separator = ':' }) }
        local merged = priority.merge(base, { paths = priority.before('/opt/bin') })
        merged = priority.merge(merged, { paths = priority.default('/usr/bin') })
        merged = priority.merge(merged, { paths = priority.after('/usr/local/bin') })
        local resolved = priority.resolve(merged)
        assert(resolved.paths == '/opt/bin:/usr/bin:/usr/local/bin', 
          'paths should merge: ' .. tostring(resolved.paths))
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn mergeable_array_concatenates() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local base = { packages = priority.mergeable() }
        local merged = priority.merge(base, { packages = priority.before({'vim'}) })
        merged = priority.merge(merged, { packages = priority.after({'emacs'}) })
        local resolved = priority.resolve(merged)
        assert(resolved.packages[1] == 'vim', 'first should be vim')
        assert(resolved.packages[2] == 'emacs', 'second should be emacs')
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn source_tracking() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local wrapped = priority.force('test')
        assert(wrapped.__source, 'should have source')
        assert(wrapped.__source.file, 'should have file')
        assert(type(wrapped.__source.line) == 'number', 'should have line number')
      "#,
    )
    .exec()?;

  Ok(())
}

#[test]
fn conflict_error_includes_source_locations() -> LuaResult<()> {
  let (lua, _) = create_test_runtime()?;

  let result = lua
    .load(
      r#"
        local priority = require('syslua.priority')
        local base = { port = priority.default(8080) }
        local override = { port = priority.default(9090) }
        priority.merge(base, override)
      "#,
    )
    .exec();

  assert!(result.is_err());
  let err = result.unwrap_err().to_string();
  assert!(err.contains("File:"), "error should include file location: {}", err);
  assert!(
    err.contains("Resolution options"),
    "error should include resolution options: {}",
    err
  );

  Ok(())
}
