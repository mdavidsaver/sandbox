#include <linux/capability.h>

// glibc includes the syscall wrappers, but doesn't make them public.
// However, libcap has been depending on these for some time.
int capset(cap_user_header_t header, cap_user_data_t data);
int capget(cap_user_header_t header, const cap_user_data_t data);
