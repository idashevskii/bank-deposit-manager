use core::hash::Hash;
use std::cmp::Ordering;
use std::collections::HashMap;

pub fn order_by<'a, T, F>(vec: &Vec<&'a T>, compare: F) -> Vec<&'a T>
where
    F: Fn(&T, &T) -> Ordering,
{
    let mut ret = vec.clone();
    ret.sort_by(|&a, &b| compare(a, b));
    ret
}

pub fn group_by<'a, 'k, T, F, K>(vec: &Vec<&'a T>, func: F) -> HashMap<&'k K, Vec<&'a T>>
where
    'a: 'k,
    K: Eq + Hash,
    F: Fn(&T) -> &K,
{
    let mut ret = HashMap::new();
    for &item in vec {
        let key = func(item);
        if !ret.contains_key(&key) {
            ret.insert(key, vec![item]);
        } else {
            ret.get_mut(&key).unwrap().push(item);
        }
    }

    return ret;
}

pub fn index_by<'a, 'k, T, F, K>(vec: &Vec<&'a T>, func: F) -> HashMap<&'k K, &'a T>
where
    'a: 'k,
    K: Eq + Hash,
    F: Fn(&T) -> &K,
{
    let mut ret = HashMap::new();
    for &item in vec {
        let key = func(item);
        ret.insert(key, item);
    }
    return ret;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_by() {
        let v_own = vec![(1, 1), (2, 1), (1, 2), (2, 2)];
        let v = v_own.iter().collect();

        let indexed = order_by(&v, |(k1, _), (k2, _)| k1.cmp(k2));

        assert_eq!(*indexed[0], (1, 1));
        assert_eq!(*indexed[1], (1, 2));
        assert_eq!(*indexed[2], (2, 1));
        assert_eq!(*indexed[3], (2, 2));
    }

    #[test]
    fn test_index_by() {
        let v_own = vec![(1, 1), (2, 2), (3, 3), (4, 4)];
        let v = v_own.iter().collect();

        let indexed = index_by(&v, |(k, _)| k);

        assert_eq!(**(indexed.get(&1).unwrap()), (1, 1));
        assert_eq!(**(indexed.get(&2).unwrap()), (2, 2));
        assert_eq!(**(indexed.get(&3).unwrap()), (3, 3));
        assert_eq!(**(indexed.get(&4).unwrap()), (4, 4));
    }

    #[test]
    fn test_group_by() {
        let v_own = vec![(1, 1), (1, 2), (2, 1), (2, 2)];
        let v = v_own.iter().collect();

        let grouped = group_by(&v, |(k, _)| k);

        assert_eq!(*(grouped.get(&1).unwrap()[0]), (1, 1));
        assert_eq!(*(grouped.get(&1).unwrap()[1]), (1, 2));
        assert_eq!(*(grouped.get(&2).unwrap()[0]), (2, 1));
        assert_eq!(*(grouped.get(&2).unwrap()[1]), (2, 2));
    }
}
