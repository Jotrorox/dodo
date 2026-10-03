async function eventually(label, check, { timeoutMs = 15000, intervalMs = 50 } = {}) {
  const deadline = Date.now() + timeoutMs;
  let cancellation;
  while (Date.now() < deadline) {
    try {
      const value = await check();
      if (value) return value;
    } catch (error) {
      // VS Code can cancel editor/provider requests during startup or restart.
      // Keep the readiness deadline and propagate every other failure.
      if (error?.name !== "Canceled" && error?.name !== "CancellationError") throw error;
      cancellation = error;
    }
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
  throw new Error(`Timed out waiting for ${label}`, { cause: cancellation });
}

module.exports = { eventually };
