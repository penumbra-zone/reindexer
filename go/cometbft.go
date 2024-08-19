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

//export c_store_first_height
func c_store_first_height(ptr unsafe.Pointer) C.long {
	return C.long(cgo.Handle(uintptr(ptr)).Value().(*store.Store).FirstHeight())
}

//export c_store_last_height
func c_store_last_height(ptr unsafe.Pointer) C.long {
	return C.long(cgo.Handle(uintptr(ptr)).Value().(*store.Store).LastHeight())
}

//export c_store_block_by_height
func c_store_block_by_height(ptr unsafe.Pointer, height C.long, out unsafe.Pointer, out_cap C.int) C.int {
	go_height := int64(height)
	go_out := unsafe.Slice((*byte)(out), int(out_cap))
	res, err := cgo.Handle(uintptr(ptr)).Value().(*store.Store).BlockByHeight(go_height, go_out)
	if err != nil {
		panic(err)
	}
	return C.int(res)
}

//export c_store_delete
func c_store_delete(ptr unsafe.Pointer) {
	cgo.Handle(uintptr(ptr)).Delete()
}

func main() {}
