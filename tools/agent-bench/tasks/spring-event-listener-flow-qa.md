In Spring Framework, trace what happens between a call to
`ApplicationEventPublisher.publishEvent(Object)` on a refreshed annotation-based
application context and the invocation of a bean method annotated with
`@EventListener`. Only the core container matters: stay within the
`spring-core`, `spring-beans` and `spring-context` modules and describe the
default setup (no custom infrastructure beans).

I need a written answer I can use as a debugging map. Use the real class and
method names from the source, not general descriptions. Cover, in this order:

1. Startup discovery. Which class finds `@EventListener` methods, in which of
   its callback methods, and which bean-factory method triggers that callback.
   Which factory class turns such a method into a listener, which listener
   class it creates, and how that listener is handed to the context. Also name
   the class that registers these infrastructure beans in the first place.
2. Publication. The ordered call chain from `publishEvent(Object)` down to the
   reflective call of the listener method, one hop per line as `Class.method`.
   Say what happens to an event object that is not an `ApplicationEvent`, and
   in which field events published before the dispatching infrastructure is
   ready are held and from which method they are replayed.
3. Listener selection. Which methods decide whether a listener receives a
   given event, where the lookup result is cached, and what the cache key is
   made of.
4. Sync or async. The exact condition under which a listener runs on another
   thread, what the default is, and what happens when the executor refuses the
   task.
5. Failure. What happens to an exception thrown by the listener method, by
   default and when error handling is configured (name the type involved).
6. Return value. Which method handles a non-null return value of the listener
   method, and what it does with a plain object, with an array or collection,
   and with an asynchronous result type.

Do not modify any file. Put the complete answer in the `summary` field of your
final response.
