use risc0_zkvm::guest::env;

fn main() {
  let iterations: u32 = env::read();
  let answer = fibonacci(iterations);
  env::commit(&answer);
}

/// Implementation of the nth Fibonacci number calculation.
///
/// This implementation a straight-forward, closely following the standard description of the
/// algorithm. It provides the baseline for comparison with any optimizations.
///
/// NOTE: Marked with #[inline(never)] to make sure this function is easy to see in the profile.
#[inline(never)]
pub fn fibonacci(n: u32) -> u64 {
  let (mut a, mut b) = (0, 1);
  if n <= 1 {
    return n as u64;
  }

  let mut i = 2;
  while i <= n {
    let c = a + b;
    a = b;
    b = c;
    i += 1;
  }

  b
}
