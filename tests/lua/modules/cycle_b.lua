-- fixture: sees the partially initialised cycle_a
local a = require("cycle_a")
return {saw_partial = a.partial}
