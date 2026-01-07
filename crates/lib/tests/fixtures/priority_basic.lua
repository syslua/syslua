local priority = require('syslua.priority')

local function test_wrap_and_unwrap()
  local wrapped = priority.wrap(42, 500)
  assert(priority.is_priority(wrapped), 'wrapped should be priority')
  assert(priority.unwrap(wrapped) == 42, 'unwrap should return raw value')
  assert(priority.get_priority(wrapped) == 500, 'get_priority should return 500')
end

local function test_helpers()
  local f = priority.force('val')
  local b = priority.before('val')
  local d = priority.default('val')
  local a = priority.after('val')
  local o = priority.order(750, 'val')

  assert(priority.get_priority(f) == 50, 'force should be 50')
  assert(priority.get_priority(b) == 500, 'before should be 500')
  assert(priority.get_priority(d) == 1000, 'default should be 1000')
  assert(priority.get_priority(a) == 1500, 'after should be 1500')
  assert(priority.get_priority(o) == 750, 'order(750) should be 750')
end

local function test_plain_values()
  assert(not priority.is_priority(42), 'plain number should not be priority')
  assert(priority.unwrap(42) == 42, 'unwrap plain should passthrough')
  assert(priority.get_priority(42) == 1000, 'plain should have default priority')
end

local function test_mergeable()
  local m = priority.mergeable({ separator = ':' })
  assert(priority.is_mergeable(m), 'should be mergeable')
  assert(m.separator == ':', 'separator should be :')

  local m2 = priority.mergeable()
  assert(priority.is_mergeable(m2), 'should be mergeable without opts')
  assert(m2.separator == nil, 'separator should be nil')
end

local function test_merge_lower_priority_wins()
  local base = { port = priority.default(8080) }
  local override = { port = priority.before(9090) }
  local result = priority.merge(base, override)
  assert(result.port == 9090, 'before(9090) should beat default(8080)')
end

local function test_merge_force_wins()
  local base = { port = priority.before(8080) }
  local override = { port = priority.force(443) }
  local result = priority.merge(base, override)
  assert(result.port == 443, 'force(443) should beat before(8080)')
end

local function test_merge_same_priority_same_value()
  local base = { port = priority.default(8080) }
  local override = { port = priority.default(8080) }
  local result = priority.merge(base, override)
  assert(result.port == 8080, 'same value same priority should work')
end

local function test_mergeable_string()
  local base = {
    paths = priority.mergeable({ separator = ':' }),
  }
  local merged = priority.merge(base, { paths = priority.before('/opt/bin') })
  merged = priority.merge(merged, { paths = priority.default('/usr/bin') })
  merged = priority.merge(merged, { paths = priority.after('/usr/local/bin') })
  assert(merged.paths == '/opt/bin:/usr/bin:/usr/local/bin', 'paths should merge with separator')
end

local function test_mergeable_array()
  local base = {
    packages = priority.mergeable(),
  }
  local merged = priority.merge(base, { packages = priority.before({ 'vim' }) })
  merged = priority.merge(merged, { packages = priority.after({ 'emacs' }) })
  assert(merged.packages[1] == 'vim', 'first should be vim (before)')
  assert(merged.packages[2] == 'emacs', 'second should be emacs (after)')
end

local function test_source_tracking()
  local wrapped = priority.force('test')
  assert(wrapped.__source, 'should have source')
  assert(wrapped.__source.file, 'should have file')
  assert(wrapped.__source.line, 'should have line')
end

test_wrap_and_unwrap()
test_helpers()
test_plain_values()
test_mergeable()
test_merge_lower_priority_wins()
test_merge_force_wins()
test_merge_same_priority_same_value()
test_mergeable_string()
test_mergeable_array()
test_source_tracking()

return { success = true }
