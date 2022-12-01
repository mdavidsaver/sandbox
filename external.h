#include <linux/capability.h>
#include <linux/if_tun.h>
#include <linux/if_bridge.h>
#include <linux/sockios.h>
#include <sys/ioctl.h>
#include <net/if.h>

// cf. https://github.com/rust-lang/rust-bindgen/issues/753
static const unsigned int REAL_TUNSETIFF = TUNSETIFF;

// glibc includes the syscall wrappers, but doesn't make them public.
// However, libcap has been depending on these for some time.
int capset(cap_user_header_t header, cap_user_data_t data);
int capget(cap_user_header_t header, const cap_user_data_t data);
