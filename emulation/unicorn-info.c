#include <stdio.h>
#include <unicorn/unicorn.h>

int main(void)
{
    unsigned int major = 0;
    unsigned int minor = 0;
    uc_engine *engine = NULL;
    uc_version(&major, &minor);
    if (major != UC_API_MAJOR || minor != UC_API_MINOR) {
        fprintf(stderr, "Unicorn headers and runtime API versions differ\n");
        return 1;
    }

    /* Synthetic ARM engine initialization; no X-M1 code or device access. */
    uc_err error = uc_open(UC_ARCH_ARM, UC_MODE_ARM, &engine);
    if (error != UC_ERR_OK) {
        fprintf(stderr, "Unicorn initialization: %s\n", uc_strerror(error));
        return 1;
    }
    error = uc_close(engine);
    if (error != UC_ERR_OK) {
        fprintf(stderr, "Unicorn cleanup: %s\n", uc_strerror(error));
        return 1;
    }
    printf("Unicorn API %u.%u: C interface ready (no target fixture run)\n", major, minor);
    return 0;
}
