local priority = require('syslua.priority')
local packages = require('syslua.environment.packages')

local function test_module_exports()
  assert(packages.setup, 'packages module should export setup function')
  assert(packages.opts, 'packages module should export opts table')
end

local function test_setup_requires_use_field()
  local success, err = pcall(function()
    packages.setup({})
  end)
  assert(not success, 'setup without use should fail')
  assert(err:match('use'), 'error should mention "use" field')
end

local function test_setup_requires_nonempty_use()
  local success, err = pcall(function()
    packages.setup({ use = {} })
  end)
  assert(not success, 'setup with empty use should fail')
  assert(err:match('use'), 'error should mention "use" field')
end

local function test_default_link_options()
  assert(packages.opts.link.bin == true, 'bin should default to true')
  assert(packages.opts.link.man == true, 'man should default to true')
  assert(packages.opts.link.completions == true, 'completions should default to true')
  assert(packages.opts.link.lib == false, 'lib should default to false')
  assert(packages.opts.link.include == false, 'include should default to false')
end

local function test_default_shell_integration()
  assert(packages.opts.shell_integration == true, 'shell_integration should default to true')
end

local function create_mock_build(id, outputs)
  return {
    id = id,
    hash = id .. '_hash_12345678',
    outputs = outputs or {},
  }
end

local function test_priority_resolution_force_wins()
  local pkg1 = create_mock_build('vim', { bin = '/store/vim/bin/vim' })
  local pkg2 = create_mock_build('nvim', { bin = '/store/nvim/bin/vim' })

  local use_list = {
    priority.default(pkg1),
    priority.force(pkg2),
  }

  assert(priority.get_priority(use_list[1]) == 1000, 'default should be 1000')
  assert(priority.get_priority(use_list[2]) == 50, 'force should be 50')
end

local function test_priority_resolution_before_beats_plain()
  local pkg1 = create_mock_build('ripgrep', { bin = '/store/rg/bin/rg' })
  local pkg2 = create_mock_build('grep', { bin = '/store/grep/bin/rg' })

  local use_list = {
    pkg1,
    priority.before(pkg2),
  }

  assert(priority.get_priority(use_list[1]) == 900, 'plain should be 900')
  assert(priority.get_priority(use_list[2]) == 500, 'before should be 500')
end

test_module_exports()
test_setup_requires_use_field()
test_setup_requires_nonempty_use()
test_default_link_options()
test_default_shell_integration()
test_priority_resolution_force_wins()
test_priority_resolution_before_beats_plain()

return { success = true }
