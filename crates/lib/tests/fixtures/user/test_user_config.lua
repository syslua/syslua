-- Minimal user config for testing
local env = require('syslua.environment')

env.variables.setup({
  TEST_USER_VAR = 'hello from user config',
})
