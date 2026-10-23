// macOS-only test interposer; never linked into the application. Uses dyld's original-call exemption.
#include <cstdlib>
#include <cstdint>
#include <pthread.h>
namespace {thread_local bool enabled=false;thread_local uint64_t allocs=0,frees=0,locks=0;}
struct AuditCounts {uint64_t allocations,deallocations,blockingLocks;};
extern "C" void od_rt_audit_begin(){allocs=frees=locks=0;enabled=true;}
extern "C" AuditCounts od_rt_audit_end(){enabled=false;return {allocs,frees,locks};}
static void* auditMalloc(size_t n){if(enabled)++allocs;return malloc(n);}
static void* auditCalloc(size_t n,size_t s){if(enabled)++allocs;return calloc(n,s);}
static void* auditRealloc(void* p,size_t n){if(enabled){++allocs;if(p)++frees;}return realloc(p,n);}
static int auditAligned(void** p,size_t a,size_t n){if(enabled)++allocs;return posix_memalign(p,a,n);}
static void auditFree(void* p){if(enabled&&p)++frees;free(p);}
static int auditMutex(pthread_mutex_t* p){if(enabled)++locks;return pthread_mutex_lock(p);}
static int auditRead(pthread_rwlock_t* p){if(enabled)++locks;return pthread_rwlock_rdlock(p);}
static int auditWrite(pthread_rwlock_t* p){if(enabled)++locks;return pthread_rwlock_wrlock(p);}
#define INTERPOSE(replacement,original) __attribute__((used)) static const struct {const void* replacement;const void* original;} pair_##original __attribute__((section("__DATA,__interpose")))={reinterpret_cast<const void*>(&replacement),reinterpret_cast<const void*>(&original)};
INTERPOSE(auditMalloc,malloc)
INTERPOSE(auditCalloc,calloc)
INTERPOSE(auditRealloc,realloc)
INTERPOSE(auditAligned,posix_memalign)
INTERPOSE(auditFree,free)
INTERPOSE(auditMutex,pthread_mutex_lock)
INTERPOSE(auditRead,pthread_rwlock_rdlock)
INTERPOSE(auditWrite,pthread_rwlock_wrlock)
