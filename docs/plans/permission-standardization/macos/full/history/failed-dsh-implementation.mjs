export function multiply(a, b) {
  if (!Number.isSafeInteger(a)) {
    throw new TypeError('a must be a safe integer');
  }
  if (!Number.isSafeInteger(b)) {
    throw new TypeError('b must be a safe integer');
  }

  const result = a * b;
  if (!Number.isSafeInteger(result)) {
    throw new RangeError('The product must be a safe integer');
  }

  return result;
}
