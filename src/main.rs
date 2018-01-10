extern crate serial;
extern crate uuid;
extern crate byteorder;

use std::io::Cursor;
use byteorder::{BigEndian, ReadBytesExt};
use uuid::Uuid;
use std::iter::Iterator;
use std::time::Duration;
#[allow(unused_imports)]
use std::io::prelude::*;
use serial::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::Arc;

/// Тип соответствует представлению последовательного порта
type ISerialPort = Rc<RefCell<SerialPort>>;

/// Тип представляет из себя UUID
type IGUID = String;

/// Расход счётчиков
#[derive(Debug, Clone, Copy)]
struct IConsumption(i32);

/// Данные для передачи в канал связи
type ISerialData = Vec<u8>;

/// Дочерний элемент для канала связи
type ISerialPortChild = ICounter<GUID = IGUID, Consumption = IConsumption>;

/// Родительский элемент для счётчика
type ICounterParent = ILinkChannel<Data=ISerialData, Child=ISerialPortChild>;

/// # Типаж канала связи
/// 
trait ILinkChannel {
    type Data;
    type Child;
    /// Конструктор
    fn new() -> Self
    where
        Self: Sized;
    /// Настройка канала связи
    fn reconf(&mut self);
    /// Отправить данные
    fn send(&mut self, data: &Self::Data);
    /// Прочитать данные
    fn read(&mut self) -> Self::Data;
    /// Обмен данными программируется в этом методе
    fn processing(&mut self);
    /// Добавить дочерний элемент
    fn new_child(&mut self, child: Self::Child);
    /// Список дочерних элементов
    fn childs(&self) -> Vec<Self::Child>;
}

trait ICounter {
    /// Тип расхода может быть разным, но от этого он не перестаёт быть расходом
    type Consumption;
    type GUID;
    /// Конструктор
    fn new(channel: Arc<ICounterParent>) -> Self
    where
        Self: Sized;
    /// Уникальный GUID устройства
    fn guid(&mut self) -> Self::GUID
    where
        Self: Sized;
    /// Добавление в канал связи команд
    fn communicate(&self);
    /// Обработка ответов
    fn processing(&mut self, request: Vec<u8>, response: Vec<u8>);
    /// Вернуть расход
    fn consumption(&self) -> Self::Consumption;
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
    fn parent(&self) -> Arc<ICounterParent>;
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

struct SerialChannel<'a> {
    port: Option<ISerialPort>,
    port_name: String,
    baud_rate: serial::BaudRate,
    _child: Vec<&'a ISerialPortChild>,
}

impl<'a> ILinkChannel for SerialChannel<'a> {
    type Data = ISerialData;
    type Child = &'a ISerialPortChild;

    fn new() -> SerialChannel<'a> {
        SerialChannel {
            port: None,
            port_name: "COM1".to_owned(),
            baud_rate: serial::Baud9600,
            _child: vec![],
        }
    }

    fn reconf(&mut self) {
        self.port = Some(Rc::new(
            RefCell::new(serial::open(&self.port_name).unwrap()),
        ));

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

    fn send(&mut self, data: &Self::Data) {
        if let Some(ref mut port) = self.port {
            let _ = port.borrow_mut().write(&data[..]).unwrap();
        }
    }

    fn read(&mut self) -> Self::Data {
        let mut result: Vec<u8> = (0..255).collect();

        if let Some(ref mut port) = self.port {
            let reading = port.borrow_mut().read(&mut result[..]).unwrap();
            result.truncate(reading);
        };
        result
    }

    fn processing(&mut self) {
        // Перенастроем порт
        self.reconf();

        // Соберём данные для отправки
        let buf: Self::Data = (0..255).collect();

        // Отправим пакет
        self.send(&buf);

        // Прочитаем ответ
        let _ = self.read();
    }

    // Добавить дочерний элемент
    fn new_child(&mut self, child: Self::Child) {
        self._child.push(child);
    }

    // Список дочерних элементов
    fn childs(&self) -> Vec<Self::Child> {
        self._child.clone()
    }
}

struct IMercury230 {
    _parent: Arc<ICounterParent>,
    _consumption: f64,
    guid: IGUID,
}

impl ICounter for IMercury230 {

    type Consumption = f64;
    type GUID = IGUID;

    // Конструктор
    fn new(channel: Arc<ICounterParent>) -> Self {
        IMercury230 {
            _parent: channel,
            _consumption: 0.0,
            guid: String::new(),
        }
    }

    // Уникальный GUID устройства
    fn guid(&mut self) -> Self::GUID {
        if self.guid.is_empty() {
            self.guid = format!("{}", Uuid::new_v4());        
        }
        format!("{}", &self.guid)
    }

    // Добавление в канал связи команд
    fn communicate(&self) {

    }

    // Обработка ответов
    fn processing(&mut self, request: Vec<u8>, response: Vec<u8>) {

        match (request[2], request[3]) {
            (5, 0) => { // Был запрос о расходе
                let tariff = request[4];                
                let mut rdr = Cursor::new(vec![ response[4], response[5], response[2], response[3] ]);
                self._consumption = rdr.read_f64::<BigEndian>().unwrap() / 1000.0;
                println!("Тариф: {} - Расход: {}", tariff, self._consumption);
            },
            _ => (),
        }
    }

    // Вернуть расход
    fn consumption(&self) -> Self::Consumption {
        self._consumption
    }

    // Тип счётчика
    fn type_name() -> &'static str {
        "IMercury230"
    }

    // Имя счётчика
    fn name(&self) -> Option<String> {
        None
    }

    // Серийный номер
    fn serial(&self) -> Option<String> {
        None
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
    fn set_verification_interval(&mut self, interval: Duration) -> std::io::Result<()> {
        Ok(())
    }

    // Вернуть канал связи
    fn parent(&self) -> Arc<ICounterParent> {
        self._parent.clone()
    }
}

impl IElectroCounter for IMercury230 {

    type Energy = f64;
    type Phase = i32;
    type Voltage = f32;

    // Активная энергия
    fn active_energy(&self, phase: Self::Phase) -> Option<Self::Energy> {
        None
    }

    // Реактивная энергия
    fn reactive_energy(&self, phase: Self::Phase) -> Option<Self::Energy> {
        None
    }

    // Действующие значения фазных токов
    fn voltage(&self, phase: Self::Phase) -> Option<Self::Voltage> {
        None
    }

    // Частота сети
    fn frequencies(&self, phase: Self::Phase) -> Option<i32> {
        None
    }
    
}

impl IFaceMercury230 for IMercury230 {

}

fn main() {
    let _ = SerialChannel::new();
}
