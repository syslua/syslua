-- Test fixture for syslua.user module
-- Note: This requires elevated privileges and creates real users
-- Only run in isolated test environments

local user = require('syslua.user')

user.setup({
  testuser = {
    description = 'Test User',
    homeDir = sys.os == 'windows' and 'C:\\Users\\testuser' or '/home/testuser',
    config = './test_user_config.lua',
    groups = {},
    preserveHomeOnRemove = true,
  },
})
