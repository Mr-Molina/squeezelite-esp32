#include <new>
#include <memory>
#include <esp_heap_caps.h>

void* operator new(size_t size) {
    void *p = heap_caps_malloc(size, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (!p) p = malloc(size);
    if (!p) throw std::bad_alloc();
    return p;
}

void* operator new(size_t size, const std::nothrow_t&) noexcept {
    void *p = heap_caps_malloc(size, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (!p) p = malloc(size);
    return p;
}

void operator delete(void* ptr) noexcept { 
	if (ptr) free(ptr); 
}

/*
// C++17 only
void* operator new (std::size_t count, std::align_val_t alignment) { 
	return heap_caps_malloc(count, MALLOC_CAP_SPIRAM); 
} 

// C++17 only
void operator delete(void* ptr, std::align_val_t alignment) noexcept { 
	if (ptr) free(ptr); 
}
*/
