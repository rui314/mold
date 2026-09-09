use rayon::prelude::*;

/// Stably partitions a slice, returning the number of matching elements.
/// Each block scatters into disjoint ranges computed from its match count.
pub(crate) fn stable_partition<T: Copy + Send + Sync>(
    values: &mut [T],
    pred: impl Fn(&T) -> bool + Sync,
) -> usize {
    const BLOCK: usize = 16384;
    let matches: Vec<bool> = if values.len() <= BLOCK {
        values.iter().map(&pred).collect()
    } else {
        values.par_iter().map(&pred).collect()
    };
    let counts: Vec<usize> = matches
        .par_chunks(BLOCK)
        .map(|chunk| chunk.iter().filter(|&&v| v).count())
        .collect();
    let num_matches = counts.iter().sum();
    if num_matches == 0 || num_matches == values.len() {
        return num_matches;
    }

    let mut output = values.to_vec();
    let (mut yes, mut no) = output.split_at_mut(num_matches);
    let parts: Vec<_> = counts
        .iter()
        .zip(matches.chunks(BLOCK))
        .map(|(&count, flags)| {
            let (matched, tail) = std::mem::take(&mut yes).split_at_mut(count);
            yes = tail;
            let (unmatched, tail) = std::mem::take(&mut no).split_at_mut(flags.len() - count);
            no = tail;
            (matched, unmatched)
        })
        .collect();
    parts
        .into_par_iter()
        .zip(values.par_chunks(BLOCK))
        .zip(matches.par_chunks(BLOCK))
        .for_each(|(((yes, no), input), flags)| {
            let (mut y, mut n) = (0, 0);
            for (&value, &matched) in input.iter().zip(flags) {
                if matched {
                    yes[y] = value;
                    y += 1;
                } else {
                    no[n] = value;
                    n += 1;
                }
            }
        });
    values
        .par_chunks_mut(BLOCK)
        .zip(output.par_chunks(BLOCK))
        .for_each(|(dst, src)| dst.copy_from_slice(src));
    num_matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_both_groups_across_blocks() {
        for len in [0, 1, 16383, 16384, 16385, 65539] {
            for divisor in [1, 2, 7, 100000] {
                let mut values: Vec<_> = (0..len).collect();
                let (yes, no): (Vec<_>, Vec<_>) =
                    values.iter().copied().partition(|v| v % divisor == 0);
                let count = stable_partition(&mut values, |v| v % divisor == 0);
                assert_eq!(count, yes.len());
                assert_eq!(&values[..count], yes);
                assert_eq!(&values[count..], no);
                assert_eq!(stable_partition(&mut values, |_| false), 0);
            }
        }
    }
}
