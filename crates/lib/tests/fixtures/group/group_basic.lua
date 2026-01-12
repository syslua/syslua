-- Test fixture for syslua.group module
-- Note: This requires elevated privileges and creates real groups
-- Only run in isolated test environments

local group = require('syslua.group')

group.setup({
  testgroup = {
    description = 'Test Group',
    gid = 2001,
  },
  sysgroup = {
    description = 'System Test Group',
    system = true,
  },
})
