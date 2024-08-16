package main

import (
	"C"
	"runtime/cgo"
	"unsafe"

	"github.com/penumbra-zone/reindexer/go/store"
)

//export c_store_new
func c_store_new(dir_ptr *C.char, dir_len C.int, backend_ptr *C.char, backend_len C.int) unsafe.Pointer {
	backend := C.GoStringN(backend_ptr, backend_len)
	dir := C.GoStringN(dir_ptr, dir_len)
	store, err := store.NewStore(backend, dir)
	if err != nil {
		panic(err)
	}
	return unsafe.Pointer(uintptr(cgo.NewHandle(store)))
}

//export c_store_height
func c_store_height(ptr unsafe.Pointer) C.long {
	return C.long(cgo.Handle(uintptr(ptr)).Value().(*store.Store).Height())
}

//export c_store_delete
func c_store_delete(ptr unsafe.Pointer) {
	cgo.Handle(uintptr(ptr)).Delete()
}

func main() {}
