//! Types and functions for working with Ruby classes.

use std::{borrow::Cow, ffi::CStr, fmt, mem::transmute, os::raw::c_int};

#[cfg(ruby_gte_3_1)]
use rb_sys::rb_cRefinement;
use rb_sys::{
    self, rb_alloc_func_t, rb_cArray, rb_cBasicObject, rb_cBinding, rb_cClass, rb_cComplex,
    rb_cDir, rb_cEncoding, rb_cEnumerator, rb_cFalseClass, rb_cFile, rb_cFloat, rb_cHash, rb_cIO,
    rb_cInteger, rb_cMatch, rb_cMethod, rb_cModule, rb_cNameErrorMesg, rb_cNilClass, rb_cNumeric,
    rb_cObject, rb_cProc, rb_cRandom, rb_cRange, rb_cRational, rb_cRegexp, rb_cStat, rb_cString,
    rb_cStruct, rb_cSymbol, rb_cThread, rb_cTime, rb_cTrueClass, rb_cUnboundMethod, rb_class2name,
    rb_class_new, rb_class_new_instance, rb_class_superclass, rb_define_alloc_func,
    rb_get_alloc_func, rb_obj_alloc, rb_undef_alloc_func, ruby_value_type, VALUE,
};

use crate::{
    error::{protect, Error},
    into_value::{ArgList, IntoValue},
    module::Module,
    object::Object,
    try_convert::TryConvert,
    typed_data::TypedData,
    value::{
        private::{self, ReprValue as _},
        NonZeroValue, ReprValue, Value,
    },
    Ruby,
};

/// A Value pointer to a RClass struct, Ruby's internal representation of
/// classes.
///
/// See the [`Module`] trait for defining instance methods and nested
/// classes/modules.
/// See the [`Object`] trait for defining singlton methods (aka class methods).
///
/// See the [`ReprValue`] trait for additional methods available on this type.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RClass(NonZeroValue);

impl RClass {
    /// Return `Some(RClass)` if `val` is a `RClass`, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{eval, RClass};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// assert!(RClass::from_value(eval("String").unwrap()).is_some());
    /// assert!(RClass::from_value(eval("Enumerable").unwrap()).is_none());
    /// assert!(RClass::from_value(eval("nil").unwrap()).is_none());
    /// ```
    #[inline]
    pub fn from_value(val: Value) -> Option<Self> {
        unsafe {
            (val.rb_type() == ruby_value_type::RUBY_T_CLASS)
                .then(|| Self(NonZeroValue::new_unchecked(val)))
        }
    }

    #[inline]
    pub(crate) unsafe fn from_rb_value_unchecked(val: VALUE) -> Self {
        Self(NonZeroValue::new_unchecked(Value::new(val)))
    }

    /// Create a new anonymous class.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{class, prelude::*, RClass};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// let class = RClass::new(class::object()).unwrap();
    /// assert!(class.is_kind_of(class::class()));
    /// ```
    pub fn new(superclass: RClass) -> Result<RClass, Error> {
        Class::new(superclass)
    }

    /// Create a new object, an instance of `self`, passing the arguments
    /// `args` to the initialiser.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{class, prelude::*};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// let s = class::string().new_instance(()).unwrap();
    /// assert!(s.is_kind_of(class::string()));
    /// assert_eq!(s.to_string(), "");
    /// ```
    pub fn new_instance<T>(self, args: T) -> Result<Value, Error>
    where
        T: ArgList,
    {
        Class::new_instance(self, args)
    }

    /// Returns the parent class of `self`.
    ///
    /// Returns `Err` if `self` can not have a parent class.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{class, eval, prelude::*};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// let klass = class::hash().superclass().unwrap();
    /// assert!(klass.equal(class::object()).unwrap());
    /// ```
    pub fn superclass(self) -> Result<Self, Error> {
        Class::superclass(self)
    }

    /// Return the name of `self`.
    ///
    /// # Safety
    ///
    /// Ruby may modify or free the memory backing the returned str, the caller
    /// must ensure this does not happen.
    ///
    /// This can be used safely by immediately calling
    /// [`into_owned`](Cow::into_owned) on the return value.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{class, eval};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// let value = class::hash();
    /// // safe as we neve give Ruby a chance to free the string.
    /// let s = unsafe { value.name() }.into_owned();
    /// assert_eq!(s, "Hash");
    /// ```
    pub unsafe fn name(&self) -> Cow<str> {
        Class::name(self)
    }
}

impl fmt::Display for RClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", unsafe { self.to_s_infallible() })
    }
}

impl fmt::Debug for RClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inspect())
    }
}

impl IntoValue for RClass {
    fn into_value_with(self, _: &Ruby) -> Value {
        self.0.get()
    }
}

impl Object for RClass {}
impl Module for RClass {}

unsafe impl private::ReprValue for RClass {}

impl ReprValue for RClass {}

impl TryConvert for RClass {
    fn try_convert(val: Value) -> Result<Self, Error> {
        match Self::from_value(val) {
            Some(v) => Ok(v),
            None => Err(Error::new(
                Ruby::get_with(val).exception_type_error(),
                format!("no implicit conversion of {} into Class", unsafe {
                    val.classname()
                },),
            )),
        }
    }
}

/// Functions available on all types representing a Ruby class.
pub trait Class: Module {
    /// The type representing an instance of the class `Self`.
    type Instance;

    /// Create a new anonymous class.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{exception, prelude::*, ExceptionClass};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// assert!(ExceptionClass::new(exception::standard_error()).is_ok());
    /// ```
    fn new(superclass: Self) -> Result<Self, Error>;

    /// Create a new object, an instance of `self`, passing the arguments
    /// `args` to the initialiser.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{exception, prelude::*};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// let s = exception::standard_error()
    ///     .new_instance(("bang!",))
    ///     .unwrap();
    /// assert!(s.is_kind_of(exception::standard_error()));
    /// ```
    fn new_instance<T>(self, args: T) -> Result<Self::Instance, Error>
    where
        T: ArgList;

    /// Create a new object, an instance of `self`, without calling the class's
    /// `initialize` method.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{exception, prelude::*};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// let s = exception::standard_error().obj_alloc().unwrap();
    /// assert!(s.is_kind_of(exception::standard_error()));
    /// ```
    fn obj_alloc(self) -> Result<Self::Instance, Error>;

    /// Returns the parent class of `self`.
    ///
    /// Returns `Err` if `self` can not have a parent class.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{class, exception, prelude::*};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// let klass = exception::exception().superclass().unwrap();
    /// assert!(klass.equal(class::object()).unwrap());
    /// ```
    fn superclass(self) -> Result<RClass, Error> {
        protect(|| unsafe {
            RClass::from_rb_value_unchecked(rb_class_superclass(self.as_rb_value()))
        })
    }

    /// Return the name of `self`.
    ///
    /// # Safety
    ///
    /// Ruby may modify or free the memory backing the returned str, the caller
    /// must ensure this does not happen.
    ///
    /// This can be used safely by immediately calling
    /// [`into_owned`](Cow::into_owned) on the return value.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{exception, prelude::*};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    ///
    /// let value = exception::standard_error();
    /// // safe as we neve give Ruby a chance to free the string.
    /// let s = unsafe { value.name() }.into_owned();
    /// assert_eq!(s, "StandardError");
    /// ```
    unsafe fn name(&self) -> Cow<str> {
        let ptr = rb_class2name(self.as_rb_value());
        let cstr = CStr::from_ptr(ptr);
        cstr.to_string_lossy()
    }

    /// Return `self` as an [`RClass`].
    fn as_r_class(self) -> RClass {
        RClass::from_value(self.as_value()).unwrap()
    }

    /// Define an allocator function for `self` using `T`'s [`Default`]
    /// implementation.
    ///
    /// In Ruby creating a new object has two steps, first the object is
    /// allocated, and then it is initialised. Allocating the object is handled
    /// by the `new` class method, which then also calls `initialize` on the
    /// newly allocated object.
    ///
    /// This does not map well to Rust, where data is allocated and initialised
    /// in a single step. For this reason most examples in this documentation
    /// show defining the `new` class method directly, opting out of the two
    /// step allocate and then initialise process. However, this means the
    /// class can't be subclassed in Ruby.
    ///
    /// Defining an allocator function allows a class be subclassed with the
    /// normal Ruby behaviour of calling the `initialize` method.
    ///
    /// Be aware when creating an instance of once of a class with an allocator
    /// function from Rust it must be done with [`Class::new_instance`] to call
    /// the allocator and then the `initialize` method.
    ///
    /// # Panics
    ///
    /// Panics if `self` and `<T as TypedData>::class()` are not the same class.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::cell::RefCell;
    ///
    /// use magnus::{
    ///     class, define_class, embed, eval, function, method, prelude::*, typed_data, wrap, Error,
    ///     RClass, TypedData, Value,
    /// };
    /// # let _cleanup = unsafe { embed::init() };
    ///
    /// #[derive(Default)]
    /// struct Point {
    ///     x: isize,
    ///     y: isize,
    /// }
    ///
    /// #[derive(Default)]
    /// #[wrap(class = "Point")]
    /// struct MutPoint(RefCell<Point>);
    ///
    /// impl MutPoint {
    ///     fn initialize(&self, x: isize, y: isize) {
    ///         let mut this = self.0.borrow_mut();
    ///         this.x = x;
    ///         this.y = y;
    ///     }
    ///
    ///     // bypasses initialize
    ///     fn create(x: isize, y: isize) -> MutPoint {
    ///         MutPoint(RefCell::new(Point { x, y }))
    ///     }
    ///
    ///     // calls initialize
    ///     fn call_new(class: RClass, x: isize, y: isize) -> Result<Value, Error> {
    ///         class.new_instance((x, y))
    ///     }
    ///
    ///     fn distance(&self, other: &MutPoint) -> f64 {
    ///         let a = self.0.borrow();
    ///         let b = other.0.borrow();
    ///         (((b.x - a.x).pow(2) + (b.y - a.y).pow(2)) as f64).sqrt()
    ///     }
    /// }
    ///
    /// let class = define_class("Point", class::object()).unwrap();
    /// class.define_alloc_func::<MutPoint>();
    /// class
    ///     .define_singleton_method("create", function!(MutPoint::create, 2))
    ///     .unwrap();
    /// class
    ///     .define_singleton_method("call_new", method!(MutPoint::call_new, 2))
    ///     .unwrap();
    /// class
    ///     .define_method("initialize", method!(MutPoint::initialize, 2))
    ///     .unwrap();
    /// class
    ///     .define_method("distance", method!(MutPoint::distance, 1))
    ///     .unwrap();
    ///
    /// let d: f64 = eval(
    ///     "class OffsetPoint < Point
    ///        def initialize(offset, x, y)
    ///          super(x + offset, y + offset)
    ///        end
    ///      end
    ///      a = Point.new(1, 1)
    ///      b = OffsetPoint.new(2, 3, 3)
    ///      a.distance(b).round(2)",
    /// )
    /// .unwrap();
    ///
    /// assert_eq!(d, 5.66);
    /// ```
    fn define_alloc_func<T>(self)
    where
        T: Default + TypedData,
    {
        extern "C" fn allocate<T: Default + TypedData>(class: RClass) -> Value {
            Ruby::get_with(class)
                .obj_wrap_as(T::default(), class)
                .as_value()
        }

        let class = T::class(&Ruby::get_with(self));
        assert!(
            class.equal(self).unwrap_or(false),
            "{} does not match {}",
            self.as_value(),
            class
        );
        unsafe {
            rb_define_alloc_func(
                self.as_rb_value(),
                Some(transmute(allocate::<T> as extern "C" fn(RClass) -> Value)),
            )
        }
    }

    #[doc(hidden)]
    #[deprecated(
        since = "0.6.0",
        note = "please use `undef_default_alloc_func` instead"
    )]
    fn undef_alloc_func(self) {
        unsafe { rb_undef_alloc_func(self.as_rb_value()) }
    }

    /// Remove the allocator function of a class if it is Ruby's default
    /// allocator function.
    ///
    /// Useful for RTypedData, where instances should not be allocated by
    /// the default allocate function. `#[derive(TypedData)]` and `#[wrap]`
    /// take care of undefining the allocator function, you do not need
    /// to use `undef_default_alloc_func` if you're using one of those.
    ///
    /// # Examples
    ///
    /// ```
    /// use magnus::{class, eval, Class};
    /// # let _cleanup = unsafe { magnus::embed::init() };
    /// let class = magnus::define_class("Point", class::object()).unwrap();
    ///
    /// class.undef_default_alloc_func();
    ///
    /// let instance = class.new_instance(());
    /// assert_eq!(
    ///     "allocator undefined for Point",
    ///     instance.err().unwrap().to_string()
    /// );
    /// ```
    fn undef_default_alloc_func(self) {
        static INIT: std::sync::Once = std::sync::Once::new();
        static mut RB_CLASS_ALLOCATE_INSTANCE: rb_alloc_func_t = None;
        let rb_class_allocate_instance = unsafe {
            INIT.call_once(|| {
                RB_CLASS_ALLOCATE_INSTANCE =
                    rb_get_alloc_func(Ruby::get_unchecked().class_object().as_rb_value());
            });
            RB_CLASS_ALLOCATE_INSTANCE
        };

        unsafe {
            if rb_get_alloc_func(self.as_rb_value()) == rb_class_allocate_instance {
                rb_undef_alloc_func(self.as_rb_value())
            }
        }
    }
}

impl Class for RClass {
    type Instance = Value;

    fn new(superclass: Self) -> Result<Self, Error> {
        debug_assert_value!(superclass);
        let superclass = superclass.as_rb_value();
        protect(|| unsafe { Self::from_rb_value_unchecked(rb_class_new(superclass)) })
    }

    fn new_instance<T>(self, args: T) -> Result<Self::Instance, Error>
    where
        T: ArgList,
    {
        let args = args.into_arg_list_with(&Ruby::get_with(self));
        let slice = args.as_ref();
        unsafe {
            protect(|| {
                Value::new(rb_class_new_instance(
                    slice.len() as c_int,
                    slice.as_ptr() as *const VALUE,
                    self.as_rb_value(),
                ))
            })
        }
    }

    fn obj_alloc(self) -> Result<Self::Instance, Error> {
        unsafe { protect(|| Value::new(rb_obj_alloc(self.as_rb_value()))) }
    }

    fn as_r_class(self) -> RClass {
        self
    }
}

#[allow(missing_docs)]
impl Ruby {
    #[inline]
    pub fn class_array(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cArray) }
    }

    #[inline]
    pub fn class_basic_object(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cBasicObject) }
    }

    #[inline]
    pub fn class_binding(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cBinding) }
    }

    #[inline]
    pub fn class_class(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cClass) }
    }

    #[inline]
    pub fn class_complex(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cComplex) }
    }

    #[inline]
    pub fn class_dir(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cDir) }
    }

    #[inline]
    pub fn class_encoding(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cEncoding) }
    }

    #[inline]
    pub fn class_enumerator(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cEnumerator) }
    }

    #[inline]
    pub fn class_false_class(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cFalseClass) }
    }

    #[inline]
    pub fn class_file(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cFile) }
    }

    #[inline]
    pub fn class_float(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cFloat) }
    }

    #[inline]
    pub fn class_hash(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cHash) }
    }

    #[inline]
    pub fn class_io(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cIO) }
    }

    #[inline]
    pub fn class_integer(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cInteger) }
    }

    #[inline]
    pub fn class_match_class(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cMatch) }
    }

    #[inline]
    pub fn class_method(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cMethod) }
    }

    #[inline]
    pub fn class_module(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cModule) }
    }

    #[inline]
    pub fn class_name_error_mesg(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cNameErrorMesg) }
    }

    #[inline]
    pub fn class_nil_class(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cNilClass) }
    }

    #[inline]
    pub fn class_numeric(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cNumeric) }
    }

    #[inline]
    pub fn class_object(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cObject) }
    }

    #[inline]
    pub fn class_proc(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cProc) }
    }

    #[inline]
    pub fn class_random(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cRandom) }
    }

    #[inline]
    pub fn class_range(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cRange) }
    }

    #[inline]
    pub fn class_rational(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cRational) }
    }

    #[cfg(any(ruby_gte_3_1, docsrs))]
    #[cfg_attr(docsrs, doc(cfg(ruby_gte_3_1)))]
    #[inline]
    pub fn class_refinement(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cRefinement) }
    }

    #[inline]
    pub fn class_regexp(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cRegexp) }
    }

    #[inline]
    pub fn class_stat(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cStat) }
    }

    #[inline]
    pub fn class_string(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cString) }
    }

    #[inline]
    pub fn class_struct_class(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cStruct) }
    }

    #[inline]
    pub fn class_symbol(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cSymbol) }
    }

    #[inline]
    pub fn class_thread(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cThread) }
    }

    #[inline]
    pub fn class_time(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cTime) }
    }

    #[inline]
    pub fn class_true_class(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cTrueClass) }
    }

    #[inline]
    pub fn class_unbound_method(&self) -> RClass {
        unsafe { RClass::from_rb_value_unchecked(rb_cUnboundMethod) }
    }
}

/// Return Ruby's `Array` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn array() -> RClass {
    get_ruby!().class_array()
}

/// Return Ruby's `BasicObject` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn basic_object() -> RClass {
    get_ruby!().class_basic_object()
}

/// Return Ruby's `Binding` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn binding() -> RClass {
    get_ruby!().class_binding()
}

/// Return Ruby's `Class` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn class() -> RClass {
    get_ruby!().class_class()
}

/// Return Ruby's `Complex` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn complex() -> RClass {
    get_ruby!().class_complex()
}

/// Return Ruby's `Dir` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn dir() -> RClass {
    get_ruby!().class_dir()
}

/// Return Ruby's `Encoding` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn encoding() -> RClass {
    get_ruby!().class_encoding()
}

/// Return Ruby's `Enumerator` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn enumerator() -> RClass {
    get_ruby!().class_enumerator()
}

/// Return Ruby's `FalseClass` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn false_class() -> RClass {
    get_ruby!().class_false_class()
}

/// Return Ruby's `File` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn file() -> RClass {
    get_ruby!().class_file()
}

/// Return Ruby's `Float` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn float() -> RClass {
    get_ruby!().class_float()
}

/// Return Ruby's `Hash` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn hash() -> RClass {
    get_ruby!().class_hash()
}

/// Return Ruby's `IO` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn io() -> RClass {
    get_ruby!().class_io()
}

/// Return Ruby's `Integer` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn integer() -> RClass {
    get_ruby!().class_integer()
}

/// Return Ruby's `MatchData` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn match_class() -> RClass {
    get_ruby!().class_match_class()
}

/// Return Ruby's `Method` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn method() -> RClass {
    get_ruby!().class_method()
}

/// Return Ruby's `Module` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn module() -> RClass {
    get_ruby!().class_module()
}

/// Return Ruby's `NameError::Message` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn name_error_mesg() -> RClass {
    get_ruby!().class_name_error_mesg()
}

/// Return Ruby's `NilClass` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn nil_class() -> RClass {
    get_ruby!().class_nil_class()
}

/// Return Ruby's `Numeric` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn numeric() -> RClass {
    get_ruby!().class_numeric()
}

/// Return Ruby's `Object` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn object() -> RClass {
    get_ruby!().class_object()
}

/// Return Ruby's `Proc` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn proc() -> RClass {
    get_ruby!().class_proc()
}

/// Return Ruby's `Random` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn random() -> RClass {
    get_ruby!().class_random()
}

/// Return Ruby's `Range` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn range() -> RClass {
    get_ruby!().class_range()
}

/// Return Ruby's `Rational` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn rational() -> RClass {
    get_ruby!().class_rational()
}

/// Return Ruby's `Refinement` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[cfg(any(ruby_gte_3_1, docsrs))]
#[cfg_attr(docsrs, doc(cfg(ruby_gte_3_1)))]
#[inline]
pub fn refinement() -> RClass {
    get_ruby!().class_refinement()
}

/// Return Ruby's `Regexp` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn regexp() -> RClass {
    get_ruby!().class_regexp()
}

/// Return Ruby's `File::Stat` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn stat() -> RClass {
    get_ruby!().class_stat()
}

/// Return Ruby's `String` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn string() -> RClass {
    get_ruby!().class_string()
}

/// Return Ruby's `Struct` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn struct_class() -> RClass {
    get_ruby!().class_struct_class()
}

/// Return Ruby's `Symbol` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn symbol() -> RClass {
    get_ruby!().class_symbol()
}

/// Return Ruby's `Thread` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn thread() -> RClass {
    get_ruby!().class_thread()
}

/// Return Ruby's `Time` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn time() -> RClass {
    get_ruby!().class_time()
}

/// Return Ruby's `TrueClass` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn true_class() -> RClass {
    get_ruby!().class_true_class()
}

/// Return Ruby's `UnboundMethod` class.
///
/// # Panics
///
/// Panics if called from a non-Ruby thread.
#[cfg(feature = "friendly-api")]
#[inline]
pub fn unbound_method() -> RClass {
    get_ruby!().class_unbound_method()
}
