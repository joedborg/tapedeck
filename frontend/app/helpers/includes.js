import { helper } from '@ember/component/helper';

/**
 * {{includes collection value}}
 *
 * Returns true if `collection` (Array, Set, or any object with `.has` / `.includes`)
 * contains `value`.
 */
export default helper(function includes([collection, value]) {
  if (!collection) return false;
  if (typeof collection.has === 'function') return collection.has(value);
  if (typeof collection.includes === 'function') return collection.includes(value);
  return false;
});
