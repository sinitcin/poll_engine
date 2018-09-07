extern crate byteorder;
extern crate crc;
extern crate serial;
extern crate uuid;

use byteorder::{BigEndian, ReadBytesExt};
use crc::crc32;
use serial::prelude::*;
use std::cell::RefCell;
#[allow(unused_imports)]
use std::io::prelude::*;
use std::io::Cursor;
use std::iter::Iterator;
use std::rc::Rc;
use std::time::Duration;
use uuid::Uuid;

/// Тип соответствует представлению последовательного порта
type ISerialPort = Rc<RefCell<SerialPort>>;

/// Тип представляет из себя UUID
type IGUID = String;

/// Расход счётчиков
type IConsumption = f64;

/// # Типаж канала связи
///
trait ILinkChannel {
    /// Конструктор
    fn new() -> Self
    where
        Self: Sized;
    /// Настройка канала связи
    fn reconf(&mut self);
    /// Отправить данные
    fn send(&mut self, data: &Vec<u8>);
    /// Прочитать данные
    fn read(&mut self) -> Vec<u8>;
}

trait ICounter {
    /// Конструктор
    fn new(channel: Rc<RefCell<ILinkChannel>>) -> Self
    where
        Self: Sized;
    /// Уникальный GUID устройства
    fn guid(&mut self) -> IGUID;
    /// Добавление в канал связи команд
    fn communicate(&mut self);
    /// Обработка ответов
    fn processing(&mut self, request: Vec<u8>, response: Vec<u8>);
    /// Вернуть расход
    fn consumption(&self) -> IConsumption;
    /// Тип счётчика
    fn type_name() -> &'static str
    where
        Self: Sized;
    /// Имя счётчика
    fn name(&self) -> Option<String>;
    /// Серийный номер
    fn serial(&self) -> Option<String>;
    /// Выполнить поверку
    fn verification(&self) -> std::io::Result<()>;
    /// Дата поверки
    fn last_verification_date(&self) -> Option<Duration>;
    /// Как часто надо делать поверку
    fn verification_interval(&self) -> Option<Duration>;
    /// Установим интервал между поверками
    fn set_verification_interval(&mut self, interval: Duration) -> std::io::Result<()>;
    /// Вернуть канал связи
    fn parent(&self) -> Rc<RefCell<ILinkChannel>>;
}

trait IElectroCounter: ICounter {
    type Energy;
    type Phase;
    type Voltage;

    // Активная энергия
    fn active_energy(&self, phase: Self::Phase) -> Option<Self::Energy>;

    // Реактивная энергия
    fn reactive_energy(&self, phase: Self::Phase) -> Option<Self::Energy>;

    // Действующие значения фазных токов
    fn voltage(&self, phase: Self::Phase) -> Option<Self::Voltage>;

    // Частота сети
    fn frequencies(&self, phase: Self::Phase) -> Option<i32>;
}

trait IFaceMercury230: IElectroCounter {}

struct SerialChannel {
    port: Option<ISerialPort>,
    port_name: String,
    baud_rate: serial::BaudRate,
    _child: Vec<Rc<RefCell<ICounter>>>,
}

impl ILinkChannel for SerialChannel {
    fn new() -> SerialChannel {
        SerialChannel {
            port: None,
            port_name: "COM1".to_owned(),
            baud_rate: serial::Baud9600,
            _child: vec![],
        }
    }

    fn reconf(&mut self) {
        self.port = Some(Rc::new(RefCell::new(
            serial::open(&self.port_name).unwrap(),
        )));

        let settings: serial::PortSettings = serial::PortSettings {
            baud_rate: self.baud_rate.clone(),
            char_size: serial::Bits8,
            parity: serial::ParityNone,
            stop_bits: serial::Stop1,
            flow_control: serial::FlowNone,
        };
        if let Some(ref mut port) = self.port {
            let _ = port.borrow_mut().configure(&settings).unwrap();
            port.borrow_mut()
                .set_timeout(Duration::from_secs(1))
                .unwrap();
        }
    }

    fn send(&mut self, data: &Vec<u8>) {
        if let Some(ref mut port) = self.port {
            let _ = port.borrow_mut().write(&data[..]).unwrap();
        }
    }

    fn read(&mut self) -> Vec<u8> {
        let mut result: Vec<u8> = (0..255).collect();

        if let Some(ref mut port) = self.port {
            let reading = port.borrow_mut().read(&mut result[..]).unwrap();
            result.truncate(reading);
        };
        result
    }
}

#[derive(Default)]
struct CounterList {
    counters: Vec<Box<RefCell<dyn ICounter>>>,
}

impl CounterList {
    fn new() -> Self {
        Self::default()
    }

    // Производим обмен со всеми счётчиками
    fn processing(&mut self) {
        for mut counter in &mut self.counters {
            if let Ok(mut counter_borrowed) = counter.try_borrow_mut() {
                counter_borrowed.communicate();
            }
        }
    }
}

struct IMercury230 {
    _parent: Rc<RefCell<ILinkChannel>>,
    _consumption: IConsumption,
    _serial: Option<String>,
    _name: Option<String>,
    guid: IGUID,
    address: u8,
}

impl ICounter for IMercury230 {
    // Конструктор
    fn new(channel: Rc<RefCell<ILinkChannel>>) -> Self {
        IMercury230 {
            _parent: channel,
            _consumption: 0.0,
            _serial: None,
            _name: None,
            guid: String::new(),
            address: 0,
        }
    }

    // Уникальный GUID устройства
    fn guid(&mut self) -> IGUID {
        if self.guid.is_empty() {
            self.guid = format!("{}", Uuid::new_v4());
        }
        format!("{}", &self.guid)
    }

    // Добавление в канал связи команд
    fn communicate(&mut self) {
        // Получаем канал связи для работы
        let parent = self.parent();
        let mut parent_borrowed = parent.borrow_mut();

        // Настройка соединения
        parent_borrowed.reconf();

        // Генерируем пакет для получения расхода
        let mut consumption = vec![self.address, 05, 00, 01];
        let my_crc = crc32::checksum_ieee(&consumption[..]);
        let mut my_crc: Vec<u8> = unsafe { Vec::from_raw_parts(my_crc as *mut u8, 4, 4) };
        consumption.append(&mut my_crc);

        // Отсылаем пакет, получаем ответ и обрабатываем
        parent_borrowed.send(&consumption);
        let question = parent_borrowed.read();

        self.processing(consumption, question);
    }

    // Обработка ответов
    fn processing(&mut self, request: Vec<u8>, response: Vec<u8>) {
        match (request[2], request[3]) {
            (5, 0) => {
                // Был запрос о расходе
                let tariff = request[4];
                let mut rdr = Cursor::new(vec![response[4], response[5], response[2], response[3]]);
                self._consumption = rdr.read_f64::<BigEndian>().unwrap() / 1000.0;
                println!(
                    "Тариф: {} - Расход: {}",
                    tariff, self._consumption
                );
            }
            _ => (),
        }
    }

    // Вернуть расход
    fn consumption(&self) -> IConsumption {
        self._consumption
    }

    // Тип счётчика
    fn type_name() -> &'static str {
        "IMercury230"
    }

    // Имя счётчика
    fn name(&self) -> Option<String> {
        self._name.clone()
    }

    // Серийный номер
    fn serial(&self) -> Option<String> {
        self._serial.clone()
    }

    // Выполнить поверку
    fn verification(&self) -> std::io::Result<()> {
        Ok(())
    }

    // Дата поверки
    fn last_verification_date(&self) -> Option<Duration> {
        None
    }

    // Как часто надо делать поверку
    fn verification_interval(&self) -> Option<Duration> {
        None
    }

    // Установим интервал между поверками
    fn set_verification_interval(&mut self, _interval: Duration) -> std::io::Result<()> {
        Ok(())
    }

    // Вернуть канал связи
    fn parent(&self) -> Rc<RefCell<ILinkChannel>> {
        self._parent.clone()
    }
}

impl IElectroCounter for IMercury230 {
    type Energy = f64;
    type Phase = i32;
    type Voltage = f32;

    // Активная энергия
    fn active_energy(&self, _phase: Self::Phase) -> Option<Self::Energy> {
        None
    }

    // Реактивная энергия
    fn reactive_energy(&self, _phase: Self::Phase) -> Option<Self::Energy> {
        None
    }

    // Действующие значения фазных токов
    fn voltage(&self, _phase: Self::Phase) -> Option<Self::Voltage> {
        None
    }

    // Частота сети
    fn frequencies(&self, _phase: Self::Phase) -> Option<i32> {
        None
    }
}

impl IFaceMercury230 for IMercury230 {}

fn main() {
    let channel = Rc::new(RefCell::new(SerialChannel::new()));
    let counter = IMercury230::new(channel.clone());
    let mut list = CounterList::new();
    println!("Hello!");
    list.counters.push(Box::new(RefCell::new(counter)));

    list.processing();

    for child in &mut list.counters {
        if let Ok(mut counter_borrowed) = child.try_borrow_mut() {
            println!("{:?}", counter_borrowed.consumption());
        }
    }
}
