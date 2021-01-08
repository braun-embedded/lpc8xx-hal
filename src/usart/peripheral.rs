use core::fmt;

use embedded_hal::{
    blocking::serial::write::Default as BlockingWriteDefault,
    serial::{Read, Write},
};
use void::Void;

use crate::{
    init_state::Disabled,
    pac::{usart0::cfg, NVIC},
    swm, syscon,
};

use super::{
    clock::{Clock, ClockSource},
    flags::{Flag, Interrupts},
    instances::Instance,
    rx::{Error, Rx},
    settings::Settings,
    state::{AsyncMode, Enabled, NoThrottle, SyncMode, Word},
    tx::Tx,
};

/// Interface to a USART peripheral
///
/// Controls the USART.  Use [`Peripherals`] to gain access to an instance of
/// this struct.
///
/// You can either use this struct as-is, if you need to send and receive in the
/// same place, or you can move the `rx` and `tx` fields out of this struct, to
/// use the sender and receiver from different contexts.
///
/// Please refer to the [module documentation] for more information.
///
/// # `embedded-hal` traits
/// - [`embedded_hal::serial::Read`] for non-blocking reads
/// - [`embedded_hal::serial::Write`] for non-blocking writes
/// - [`embedded_hal::blocking::serial::Write`] for blocking writes
///
///
/// [`Peripherals`]: ../struct.Peripherals.html
/// [module documentation]: index.html
/// [`embedded_hal::serial::Read`]: #impl-Read<W>
/// [`embedded_hal::serial::Write`]: #impl-Write<W>
/// [`embedded_hal::blocking::serial::Write`]: #impl-Write<Word>
pub struct USART<I, State> {
    /// The USART Receiver
    pub rx: Rx<I, State>,

    /// The USART Transmitter
    pub tx: Tx<I, State, NoThrottle>,

    usart: I,
}

impl<I> USART<I, Disabled>
where
    I: Instance,
{
    pub(crate) fn new(usart: I) -> Self {
        USART {
            rx: Rx::new(),
            tx: Tx::new(),

            usart,
        }
    }

    /// Enable the USART in asynchronous mode
    ///
    /// Asynchronous mode works without an external clock signal. The word
    /// "asynchronous" has no relation to blocking or non-blocking APIs, in this
    /// context.
    ///
    /// This method is only available, if `USART` is in the [`Disabled`] state.
    /// Code that attempts to call this method when the peripheral is already
    /// enabled will not compile.
    ///
    /// Consumes this instance of `USART` and returns another instance that has
    /// its `State` type parameter set to [`Enabled`].
    ///
    /// # Limitations
    ///
    /// For USART to function correctly, the UARTFRG reset must be cleared. This
    /// is the default, so unless you have messed with those settings, you
    /// should be good.
    ///
    /// # Examples
    ///
    /// Please refer to the [module documentation] for a full example.
    ///
    /// [`Disabled`]: ../init_state/struct.Disabled.html
    /// [`Enabled`]: state/struct.Enabled.html
    /// [`BaudRate`]: struct.BaudRate.html
    /// [module documentation]: index.html
    pub fn enable_async<RxPin, TxPin, CLOCK, W>(
        mut self,
        clock: &Clock<CLOCK, AsyncMode>,
        syscon: &mut syscon::Handle,
        _: swm::Function<I::Rx, swm::state::Assigned<RxPin>>,
        _: swm::Function<I::Tx, swm::state::Assigned<TxPin>>,
        settings: Settings<W>,
    ) -> USART<I, Enabled<W, AsyncMode>>
    where
        CLOCK: ClockSource,
        W: Word,
    {
        self.configure::<CLOCK>(syscon);

        self.usart
            .brg
            .write(|w| unsafe { w.brgval().bits(clock.brgval) });
        self.usart
            .osr
            .write(|w| unsafe { w.osrval().bits(clock.osrval) });

        // We are not allowed to send or receive data when writing to CFG. This
        // is ensured by type state, so no need to do anything here.

        self.usart.cfg.modify(|_, w| {
            w.syncen().asynchronous_mode();
            Self::apply_general_config(w);
            settings.apply(w);
            w
        });

        USART {
            rx: Rx::new(), // can't use `self.rx`, due to state
            tx: Tx::new(), // can't use `self.tx`, due to state
            usart: self.usart,
        }
    }

    /// Enable the USART in synchronous mode as master
    ///
    /// Synchronous mode works with an external clock signal. The word
    /// "synchronous" has no relation to blocking or non-blocking APIs, in this
    /// context.
    ///
    /// This method is only available, if `USART` is in the [`Disabled`] state.
    /// Code that attempts to call this method when the peripheral is already
    /// enabled will not compile.
    ///
    /// Consumes this instance of `USART` and returns another instance that has
    /// its `State` type parameter set to [`Enabled`].
    ///
    /// # Limitations
    ///
    /// For USART to function correctly, the UARTFRG reset must be cleared. This
    /// is the default, so unless you have messed with those settings, you
    /// should be good.
    ///
    /// [`Disabled`]: ../init_state/struct.Disabled.html
    /// [`Enabled`]: state/struct.Enabled.html
    /// [`BaudRate`]: struct.BaudRate.html
    /// [module documentation]: index.html
    pub fn enable_sync_as_master<RxPin, TxPin, SclkPin, C, W>(
        mut self,
        clock: &Clock<C, SyncMode>,
        syscon: &mut syscon::Handle,
        _: swm::Function<I::Rx, swm::state::Assigned<RxPin>>,
        _: swm::Function<I::Tx, swm::state::Assigned<TxPin>>,
        _: swm::Function<I::Sclk, swm::state::Assigned<SclkPin>>,
        settings: Settings<W>,
    ) -> USART<I, Enabled<W, SyncMode>>
    where
        C: ClockSource,
        W: Word,
    {
        self.configure::<C>(syscon);

        self.usart
            .brg
            .write(|w| unsafe { w.brgval().bits(clock.brgval) });

        // We are not allowed to send or receive data when writing to CFG. This
        // is ensured by type state, so no need to do anything here.

        self.usart.cfg.modify(|_, w| {
            w.syncen().synchronous_mode();
            w.syncmst().master();
            Self::apply_general_config(w);
            settings.apply(w);
            w
        });

        USART {
            rx: Rx::new(), // can't use `self.rx`, due to state
            tx: Tx::new(), // can't use `self.tx`, due to state
            usart: self.usart,
        }
    }

    /// Enable the USART in synchronous mode as slave
    ///
    /// Synchronous mode works with an external clock signal. The word
    /// "synchronous" has no relation to blocking or non-blocking APIs, in this
    /// context.
    ///
    /// This method is only available, if `USART` is in the [`Disabled`] state.
    /// Code that attempts to call this method when the peripheral is already
    /// enabled will not compile.
    ///
    /// Consumes this instance of `USART` and returns another instance that has
    /// its `State` type parameter set to [`Enabled`].
    ///
    /// # Limitations
    ///
    /// For USART to function correctly, the UARTFRG reset must be cleared. This
    /// is the default, so unless you have messed with those settings, you
    /// should be good.
    ///
    /// [`Disabled`]: ../init_state/struct.Disabled.html
    /// [`Enabled`]: state/struct.Enabled.html
    /// [`BaudRate`]: struct.BaudRate.html
    /// [module documentation]: index.html
    pub fn enable_sync_as_slave<RxPin, TxPin, SclkPin, C, W>(
        mut self,
        _clock: &C,
        syscon: &mut syscon::Handle,
        _: swm::Function<I::Rx, swm::state::Assigned<RxPin>>,
        _: swm::Function<I::Tx, swm::state::Assigned<TxPin>>,
        _: swm::Function<I::Sclk, swm::state::Assigned<SclkPin>>,
        settings: Settings<W>,
    ) -> USART<I, Enabled<W, SyncMode>>
    where
        C: ClockSource,
        W: Word,
    {
        self.configure::<C>(syscon);

        // We are not allowed to send or receive data when writing to CFG. This
        // is ensured by type state, so no need to do anything here.

        self.usart.cfg.modify(|_, w| {
            w.syncen().synchronous_mode();
            w.syncmst().slave();
            Self::apply_general_config(w);
            settings.apply(w);
            w
        });

        USART {
            rx: Rx::new(), // can't use `self.rx`, due to state
            tx: Tx::new(), // can't use `self.tx`, due to state
            usart: self.usart,
        }
    }

    fn configure<C>(&mut self, syscon: &mut syscon::Handle)
    where
        C: ClockSource,
    {
        syscon.enable_clock(&self.usart);
        C::select(&self.usart, syscon);

        self.usart.ctl.modify(|_, w| {
            w.txbrken().normal();
            w.addrdet().disabled();
            w.txdis().enabled();
            w.cc().continous_clock();
            w.autobaud().disabled()
        });
    }

    fn apply_general_config(w: &mut cfg::W) {
        // Enable peripheral instance.
        w.enable().enabled();

        // Disable CTS; can be enabled by the user later.
        w.ctsen().disabled();

        // No loopback mode; currently it's not supported.
        w.loop_().normal();

        // Enable automatic address matching. This makes no difference until we
        // set a separate bit in CTL, and address detection without automatic
        // matching is currently not supported by this API.
        w.autoaddr().enabled();
    }
}

impl<I, W, Mode> USART<I, Enabled<W, Mode>>
where
    I: Instance,
    W: Word,
{
    /// Disable the USART
    ///
    /// This method is only available, if `USART` is in the [`Enabled`] state.
    /// Code that attempts to call this method when the peripheral is already
    /// disabled will not compile.
    ///
    /// Consumes this instance of `USART` and returns another instance that has
    /// its `State` type parameter set to [`Disabled`].
    ///
    /// [`Enabled`]: state/struct.Enabled.html
    /// [`Disabled`]: ../init_state/struct.Disabled.html
    pub fn disable(self, syscon: &mut syscon::Handle) -> USART<I, Disabled> {
        syscon.disable_clock(&self.usart);

        USART {
            rx: Rx::new(), // can't use `self.rx`, due to state
            tx: Tx::new(), // can't use `self.tx`, due to state
            usart: self.usart,
        }
    }

    /// Query whether the provided flag is set
    ///
    /// Flags that need to be reset by software will be reset by this operation.
    pub fn is_flag_set(&self, flag: Flag) -> bool {
        flag.is_set::<I>()
    }

    /// Enable interrupts for this instance in the NVIC
    ///
    /// This only enables the interrupts in the NVIC. It doesn't enable any
    /// specific interrupt in this USART instance.
    pub fn enable_in_nvic(&mut self) {
        // Safe, because there's no critical section here that this could
        // interfere with.
        unsafe { NVIC::unmask(I::INTERRUPT) };
    }

    /// Disable interrupts for this instance in the NVIC
    ///
    /// This only disables the interrupts in the NVIC. It doesn't change
    /// anything about the interrupt configuration within this USART instance.
    pub fn disable_in_nvic(&mut self) {
        NVIC::mask(I::INTERRUPT);
    }

    /// Clear's this instance's interrupt pending flag in the NVIC
    ///
    /// This only clears the interrupt's pending flag in the NVIC. It does not
    /// affect any of the interrupt-related flags in the peripheral.
    pub fn clear_nvic_pending(&mut self) {
        NVIC::unpend(I::INTERRUPT);
    }

    /// Enable interrupts
    ///
    /// Enables all interrupts set to `true` in `interrupts`. Interrupts set to
    /// `false` are not affected.
    ///
    /// # Example
    ///
    /// ``` no_run
    /// use lpc8xx_hal::usart;
    ///
    /// # use lpc8xx_hal::Peripherals;
    /// #
    /// # let mut p = Peripherals::take().unwrap();
    /// #
    /// # let mut syscon = p.SYSCON.split();
    /// # let mut swm    = p.SWM.split();
    /// #
    /// # #[cfg(feature = "82x")]
    /// # let mut swm_handle = swm.handle;
    /// # #[cfg(feature = "845")]
    /// # let mut swm_handle = swm.handle.enable(&mut syscon.handle);
    /// #
    /// # #[cfg(feature = "82x")]
    /// # let clock_config = {
    /// #     syscon.uartfrg.set_clkdiv(6);
    /// #     syscon.uartfrg.set_frgmult(22);
    /// #     syscon.uartfrg.set_frgdiv(0xff);
    /// #     usart::Clock::new(&syscon.uartfrg, 0, 16)
    /// # };
    /// # #[cfg(feature = "845")]
    /// # let clock_config = usart::Clock::new_with_baudrate(115200);
    /// #
    /// # let (u0_rxd, _) = swm.movable_functions.u0_rxd.assign(
    /// #     p.pins.pio0_0.into_swm_pin(),
    /// #     &mut swm_handle,
    /// # );
    /// # let (u0_txd, _) = swm.movable_functions.u0_txd.assign(
    /// #     p.pins.pio0_4.into_swm_pin(),
    /// #     &mut swm_handle,
    /// # );
    /// #
    /// # let mut usart = p.USART0.enable_async(
    /// #     &clock_config,
    /// #     &mut syscon.handle,
    /// #     u0_rxd,
    /// #     u0_txd,
    /// #     usart::Settings::default(),
    /// # );
    /// #
    /// // Enable only RXRDY and TXRDY, leave other interrupts untouched.
    /// usart.enable_interrupts(usart::Interrupts {
    ///     RXRDY: true,
    ///     TXRDY: true,
    ///     .. usart::Interrupts::default()
    /// });
    /// ```
    pub fn enable_interrupts(&mut self, interrupts: Interrupts) {
        interrupts.enable::<I>();
    }

    /// Disable interrupts
    ///
    /// Disables all interrupts set to `true` in `interrupts`. Interrupts set to
    /// `false` are not affected.
    ///
    /// # Example
    ///
    /// ``` no_run
    /// use lpc8xx_hal::usart;
    ///
    /// # use lpc8xx_hal::Peripherals;
    /// #
    /// # let mut p = Peripherals::take().unwrap();
    /// #
    /// # let mut syscon = p.SYSCON.split();
    /// # let mut swm    = p.SWM.split();
    /// #
    /// # #[cfg(feature = "82x")]
    /// # let mut swm_handle = swm.handle;
    /// # #[cfg(feature = "845")]
    /// # let mut swm_handle = swm.handle.enable(&mut syscon.handle);
    /// #
    /// # #[cfg(feature = "82x")]
    /// # let clock_config = {
    /// #     syscon.uartfrg.set_clkdiv(6);
    /// #     syscon.uartfrg.set_frgmult(22);
    /// #     syscon.uartfrg.set_frgdiv(0xff);
    /// #     usart::Clock::new(&syscon.uartfrg, 0, 16)
    /// # };
    /// # #[cfg(feature = "845")]
    /// # let clock_config = usart::Clock::new_with_baudrate(115200);
    /// #
    /// # let (u0_rxd, _) = swm.movable_functions.u0_rxd.assign(
    /// #     p.pins.pio0_0.into_swm_pin(),
    /// #     &mut swm_handle,
    /// # );
    /// # let (u0_txd, _) = swm.movable_functions.u0_txd.assign(
    /// #     p.pins.pio0_4.into_swm_pin(),
    /// #     &mut swm_handle,
    /// # );
    /// #
    /// # let mut usart = p.USART0.enable_async(
    /// #     &clock_config,
    /// #     &mut syscon.handle,
    /// #     u0_rxd,
    /// #     u0_txd,
    /// #     usart::Settings::default(),
    /// # );
    /// #
    /// // Disable only RXRDY and TXRDY, leave other interrupts untouched.
    /// usart.disable_interrupts(usart::Interrupts {
    ///     RXRDY: true,
    ///     TXRDY: true,
    ///     .. usart::Interrupts::default()
    /// });
    /// ```
    pub fn disable_interrupts(&mut self, interrupts: Interrupts) {
        interrupts.disable::<I>();
    }
}

impl<I, State> USART<I, State>
where
    I: Instance,
{
    /// Return the raw peripheral
    ///
    /// This method serves as an escape hatch from the HAL API. It returns the
    /// raw peripheral, allowing you to do whatever you want with it, without
    /// limitations imposed by the API.
    ///
    /// If you are using this method because a feature you need is missing from
    /// the HAL API, please [open an issue] or, if an issue for your feature
    /// request already exists, comment on the existing issue, so we can
    /// prioritize it accordingly.
    ///
    /// [open an issue]: https://github.com/lpc-rs/lpc8xx-hal/issues
    pub fn free(self) -> I {
        self.usart
    }
}

impl<I, W, Mode> Read<W> for USART<I, Enabled<W, Mode>>
where
    I: Instance,
    W: Word,
{
    type Error = Error;

    /// Reads a single word from the serial interface
    fn read(&mut self) -> nb::Result<W, Self::Error> {
        self.rx.read()
    }
}

impl<I, W, Mode> Write<W> for USART<I, Enabled<W, Mode>>
where
    I: Instance,
    W: Word,
{
    type Error = Void;

    /// Writes a single word to the serial interface
    fn write(&mut self, word: W) -> nb::Result<(), Self::Error> {
        self.tx.write(word)
    }

    /// Ensures that none of the previously written words are still buffered
    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        self.tx.flush()
    }
}

impl<I, W, Mode> BlockingWriteDefault<W> for USART<I, Enabled<W, Mode>>
where
    I: Instance,
    W: Word,
{
}

impl<I, Mode> fmt::Write for USART<I, Enabled<u8, Mode>>
where
    Self: BlockingWriteDefault<u8>,
    I: Instance,
{
    /// Writes a string slice into this writer, returning whether the write succeeded.
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.tx.write_str(s)
    }
}
