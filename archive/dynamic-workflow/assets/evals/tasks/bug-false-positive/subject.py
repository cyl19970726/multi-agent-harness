def running_max(nums):
    """Return the largest number in nums."""
    best = nums[0]
    for x in nums[1:]:
        if x > best:
            best = x
    return best


def median(nums):
    """Return the median of nums. Does NOT modify the caller's list."""
    ordered = sorted(nums)
    n = len(ordered)
    mid = n // 2
    if n % 2 == 1:
        return ordered[mid]
    return (ordered[mid - 1] + ordered[mid]) / 2
