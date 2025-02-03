mod tcp;
mod udp;

impl<T> crate::p3::bindings::sockets::types::Host for crate::p3::sockets::WasiSocketsImpl<&mut T> where
    T: crate::p3::sockets::WasiSocketsView + 'static
{
}
