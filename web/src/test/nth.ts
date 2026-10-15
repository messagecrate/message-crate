/**
 * Item `index` of `list`, failing the test with what was there when it is
 * missing. With `noUncheckedIndexedAccess`, `list[i]` may be undefined; a test
 * that reads a row, a cell or a call by position states here that it expects
 * one, rather than going on to fail on `undefined` somewhere further down.
 */
export function nth<T>(list: ArrayLike<T>, index: number): T {
  const item = list[index];
  if (item === undefined) {
    throw new Error(`expected an item at index ${index}, but the list has ${list.length}`);
  }
  return item;
}
