local groups = require('syslua.groups')

groups.setup({
  testgroup = {
    description = 'Test Group',
    gid = 2001,
  },
  sysgroup = {
    description = 'System Test Group',
    system = true,
  },
})
