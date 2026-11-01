-- fixture: cycle_a requires cycle_b which requires cycle_a again
package.loaded["cycle_a"] = {partial = true}
local b = require("cycle_b")
return {b = b, partial = false}
