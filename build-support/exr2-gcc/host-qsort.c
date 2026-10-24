/* Keep equal keys in input order, like the container's glibc merge sort.
   GCC 4.4 and binutils have sort comparators without complete tie breakers.
   Darwin's unstable qsort can therefore change optimization choices and
   linker section order. This is host code; firmware uses none of it. */
#include <stdlib.h>
#include <string.h>

static void
merge_sort(char *base, size_t count, size_t size,
           int (*compare)(const void *, const void *), char *temporary)
{
    size_t middle = count / 2, left = 0, right = middle, output = 0;
    if (count < 2)
        return;
    merge_sort(base, middle, size, compare, temporary);
    merge_sort(base + middle * size, count - middle, size, compare, temporary);
    while (left < middle && right < count) {
        size_t index = compare(base + left * size, base + right * size) <= 0
            ? left++ : right++;
        memcpy(temporary + output++ * size, base + index * size, size);
    }
    while (left < middle)
        memcpy(temporary + output++ * size, base + left++ * size, size);
    while (right < count)
        memcpy(temporary + output++ * size, base + right++ * size, size);
    memcpy(base, temporary, count * size);
}

void
qsort(void *base, size_t count, size_t size,
      int (*compare)(const void *, const void *))
{
    char small[1024];
    char *temporary;
    size_t bytes;
    if (count < 2 || size == 0)
        return;
    if (count > (size_t)-1 / size)
        abort();
    bytes = count * size;
    temporary = bytes <= sizeof small ? small : malloc(bytes);
    /* Falling back to an unstable sort would change generated code. */
    if (temporary == NULL)
        abort();
    merge_sort(base, count, size, compare, temporary);
    if (temporary != small)
        free(temporary);
}
