extern crate byteorder;

use byteorder::{BigEndian, ByteOrder, WriteBytesExt, ReadBytesExt};
use std::io::*;

/// Default openpixel tcp port
pub const DEFAULT_OPC_PORT: usize = 7890;

const MAX_MESSAGE_SIZE: usize = 0xffff;
const SYS_EXCLUSIVE: u8 = 0xff;
const SET_PIXEL_COLORS: u8 = 0x00;
const BROADCAST_CHANNEL: u8 = 0;

/// Describes an OPC Command.
#[derive (Debug, PartialEq)]
pub enum Command<'data> {
    /// Contains and array of RGB values: three bytes in red, green, blue order for each pixel to set.
    SetPixelColors {
        /// If the data block has length 3*n, then the first n pixels of the specified channel are set.
        /// All other pixels are unaffected and retain their current colour values.
        /// If the data length is not a multiple of 3, or there is data for more pixels than are present, the extra data is ignored.
        pixels: & 'data [[u8; 3]]
    },
    /// Used to send a message that is specific to a particular device or software system.
    SystemExclusive {
        /// The data block should begin with a two-byte system ID.
        id: [u8; 2],
        /// designers of that system are then free to define any message format for the rest of the data block.
        data: & 'data [u8] }
}

/// Describes a single message that follows the OPC protocol
#[derive (Debug, PartialEq)]
pub struct Message<'data> {
    /// Up to 255 separate strands of pixels can be controlled.
    /// Channel 0 are considered broadcast messages.
    /// Channels number from 1 to 255 are for each strand and listen for messages with that channel number.
    pub channel: u8,
    /// Designates the message type
    pub command: Command<'data>
}

impl<'data> Message<'data> {
    /// Create new Message Instance
    pub fn new (ch: u8, cmd: Command<'data>) -> Message {
        Message {
            channel: ch,
            command: cmd
        }
    }

    /// Check Message Data Length
    pub fn len(&self) -> usize {
        match self.command {
            Command::SetPixelColors {ref pixels} => pixels.len()*3,
            Command::SystemExclusive {id, ref data} => data.len() + 2
        }
    }

    /// Check is Message has a valid size
    pub fn is_valid(&self) -> bool {
        self.len() <= MAX_MESSAGE_SIZE
    }

    /// Check if Message is a broadcast message
    pub fn is_broadcast(&self) -> bool {
        self.channel == BROADCAST_CHANNEL
    }
}

pub struct Client<W: Write> {
    writer: BufWriter<W>
}

impl <W: Write> Client<W> {

    pub fn new(writer: W) -> Client<W> {
        Client { writer: BufWriter::with_capacity(MAX_MESSAGE_SIZE, writer)}
    }

    pub fn send(&mut self, msg: Message) -> Result<()> {

        let ser_len = msg.len();

        match msg.command {
            Command::SetPixelColors {pixels} => {

                // Insert Channel and Command
                try!(self.writer.write(&[msg.channel, SET_PIXEL_COLORS]));
                // Insert Data Length
                try!(self.writer.write_u16::<BigEndian>(ser_len as u16));

                // Insert Data
                for pixel in pixels {
                    try!(self.writer.write(pixel));
                }
            },
            Command::SystemExclusive {id, data} => {

                // Insert Channel and Command
                try!(self.writer.write(&[msg.channel, SYS_EXCLUSIVE]));
                // Insert Data Length
                try!(self.writer.write_u16::<BigEndian>(ser_len as u16));

                // Insert Data
                try!(self.writer.write(&id));
                try!(self.writer.write(&data));
            }
        }

        self.writer.flush()
    }
}


trait Device {
    fn read_msg(&mut self, msg: &Message) -> Result<()>;
    fn channel(&self) -> u8;
}

struct Server<R: Read> {
    reader: BufReader<R>
}

impl <R: Read> Server<R> {
    fn new(reader: R) -> Server<R> {
        Server { reader: BufReader::with_capacity(MAX_MESSAGE_SIZE, reader) }
    }

    fn receive<D: Device>(&mut self, output: &mut D) -> Result<()> {

        let length = {
            let buf = try!(self.reader.fill_buf());

            // TODO: Check if buf length is more than 4;
            if buf.len() < 4 { () }
            let (channel, command) = (buf[0], buf[1]);
            let length = BigEndian::read_u16(&buf[2..4]) as usize;
            let data = &buf[4..][..length];
            match command {
                SET_PIXEL_COLORS => {
                    let pixels: Vec<_> = data[..(length-(length % 3))].chunks(3).map(|chunk| [chunk[0],chunk[1],chunk[2]]).collect();
                    output.read_msg(&Message {
                        channel: channel,
                        command: Command::SetPixelColors { pixels: &pixels }
                    });
                },
                SYS_EXCLUSIVE => {
                    output.read_msg(&Message {
                        channel: channel,
                        command: Command::SystemExclusive { id: [data[0], data[1]], data: &data[2..] }
                    });
                },
                // TODO: What to do if incorrect?
                _ => return Err(Error::new(ErrorKind::InvalidData, "Invalid Message Command"))
            }
            length+4
        };

        Ok(self.reader.consume(length))
    }

}

#[test]
fn should_create_server() {
    use std::io::*;

    let mut stream: &[u8] = &[0; 1];
    let s = Server::new(stream);
}

#[test]
fn server_should_receive_pixel_command() {

    let mut test_write = Vec::new();
    let msg = Message {
        channel: 4,
        command: Command::SetPixelColors { pixels: &[[9; 3]; 10]}
    };

    let mut client = Client::new(test_write);
    client.send(msg);

    let read_msg = client.writer.get_ref();
    println!("{:?}", &read_msg);

    struct TestDevice;
    impl Device for TestDevice {
        fn read_msg(&mut self, msg: &Message) -> Result<()> {
            assert_eq!(&Message {
                channel: 4,
                command: Command::SetPixelColors { pixels: &[[9; 3]; 10]}
            }, msg);
            Ok(())
        }
        fn channel(&self) -> u8 { 0 }
    }

    let mut s = Server::new(read_msg.as_slice());
    s.receive(&mut TestDevice {});
}

#[test]
fn server_should_receive_system_command() {

    let mut test_write = Vec::new();
    let msg = Message {
        channel: 4,
        command: Command::SystemExclusive { id: [0; 2], data: &[8; 10]}
    };

    let mut client = Client::new(test_write);
    client.send(msg);

    let read_msg = client.writer.get_ref();
    println!("{:?}", &read_msg);

    struct TestDevice;
    impl Device for TestDevice {
        fn read_msg(&mut self, msg: &Message) -> Result<()> {
            assert_eq!(&Message {
                channel: 4,
                command: Command::SystemExclusive { id: [0; 2], data: &[8; 10]}
            }, msg);
            Ok(())
        }
        fn channel(&self) -> u8 { 0 }
    }

    let mut s = Server::new(read_msg.as_slice());
    s.receive(&mut TestDevice {});
}
