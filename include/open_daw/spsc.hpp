#pragma once
#include <array>
#include <atomic>
#include <cstddef>
#include <type_traits>

namespace daw {
// Exactly one producer and one consumer. Capacity is N-1; no heap or retry loops.
template<class T, size_t N> class SpscQueue {
    static_assert(N > 1 && std::is_trivially_copyable_v<T>);
    static_assert(std::atomic<size_t>::is_always_lock_free);
    std::array<T, N> data_{};
    alignas(64) std::atomic<size_t> write_{0};
    alignas(64) std::atomic<size_t> read_{0};
public:
    bool push(const T& value) noexcept {
        const auto w = write_.load(std::memory_order_relaxed);
        const auto next = (w + 1) % N;
        if (next == read_.load(std::memory_order_acquire)) return false;
        data_[w] = value;
        write_.store(next, std::memory_order_release);
        return true;
    }
    bool pop(T& value) noexcept {
        const auto r = read_.load(std::memory_order_relaxed);
        if (r == write_.load(std::memory_order_acquire)) return false;
        value = data_[r];
        read_.store((r + 1) % N, std::memory_order_release);
        return true;
    }
    // Consumer-only emergency cancellation. Commands published afterwards remain queued.
    void discard() noexcept { read_.store(write_.load(std::memory_order_acquire), std::memory_order_release); }
};
}
