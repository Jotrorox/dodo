-- fixture module with a side effect: counts how often its chunk runs
counter_loads = (counter_loads or 0) + 1
print("counter.lua executing, load #" .. counter_loads)
return {loads = counter_loads}
