#![macro_use]

/// Returns a string to use for reading from a file, optionally with a max size.
macro_rules! file_reading_buffer {
    ($instruction: ident, $process: ident, $idx: expr) => (
        if $instruction.arguments.get($idx).is_some() {
            let size_ptr = instruction_object!($instruction, $process, $idx);
            let size_obj = size_ptr.get();

            ensure_integers!($instruction, size_obj);

            let size = size_obj.value.as_integer();

            ensure_positive_read_size!($instruction, size);

            String::with_capacity(size as usize)
        }
        else {
            String::new()
        }
    );
}
