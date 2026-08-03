package ai.chat2db.rust.compat;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.sql.Connection;
import java.util.Map;

/** Keeps H2 identifier checks on the separately supplied JDBC driver classloader. */
final class CommunityH2IdentifierCompatibility {

    private static final String H2_DATABASE_TYPE = "H2";
    private static final String PLUGIN_CLASS = "ai.chat2db.spi.IPlugin";
    private static final String METADATA_CLASS = "ai.chat2db.spi.IDbMetaData";
    private static final String IDENTIFIER_PROCESSOR_CLASS =
            "ai.chat2db.spi.ISQLIdentifierProcessor";
    private static final String H2_PARSER_UTIL_CLASS = "org.h2.util.ParserUtil";

    private final ClassLoader communityLoader;
    private final Class<?> metadataType;
    private final Class<?> identifierProcessorType;
    private final ThreadLocal<ClassLoader> driverLoader = new ThreadLocal<>();

    private CommunityH2IdentifierCompatibility(
            ClassLoader communityLoader,
            Class<?> metadataType,
            Class<?> identifierProcessorType) {
        this.communityLoader = communityLoader;
        this.metadataType = metadataType;
        this.identifierProcessorType = identifierProcessorType;
    }

    static CommunityH2IdentifierCompatibility install(
            ClassLoader communityLoader, Class<?> contextType)
            throws ReflectiveOperationException {
        Class<?> pluginType = Class.forName(PLUGIN_CLASS, true, communityLoader);
        Class<?> metadataType = Class.forName(METADATA_CLASS, true, communityLoader);
        Class<?> identifierProcessorType =
                Class.forName(IDENTIFIER_PROCESSOR_CLASS, true, communityLoader);
        CommunityH2IdentifierCompatibility compatibility =
                new CommunityH2IdentifierCompatibility(
                        communityLoader, metadataType, identifierProcessorType);

        Object value = contextType.getField("PLUGIN_MAP").get(null);
        if (!(value instanceof Map<?, ?> rawPlugins)) {
            throw new IllegalStateException("Community plugin registry is not a map");
        }
        @SuppressWarnings("unchecked")
        Map<Object, Object> plugins = (Map<Object, Object>) rawPlugins;
        Object h2Plugin = plugins.get(H2_DATABASE_TYPE);
        if (h2Plugin == null) {
            throw new IllegalStateException("Community H2 plugin is unavailable");
        }
        if (Proxy.isProxyClass(h2Plugin.getClass())) {
            InvocationHandler handler = Proxy.getInvocationHandler(h2Plugin);
            if (handler instanceof PluginHandler installed
                    && installed.compatibility.communityLoader == communityLoader) {
                return installed.compatibility;
            }
        }
        Object wrappedPlugin = Proxy.newProxyInstance(
                communityLoader,
                new Class<?>[] {pluginType},
                compatibility.pluginHandler(h2Plugin));
        plugins.put(H2_DATABASE_TYPE, wrappedPlugin);
        return compatibility;
    }

    DriverBinding bind(String databaseType, Connection connection)
            throws ReflectiveOperationException {
        if (!H2_DATABASE_TYPE.equalsIgnoreCase(databaseType)) {
            return DriverBinding.NOOP;
        }
        ClassLoader connectionLoader = connection.getClass().getClassLoader();
        if (connectionLoader == null) {
            throw new IllegalStateException("Community H2 connection has no driver classloader");
        }
        Class<?> parserUtil = Class.forName(H2_PARSER_UTIL_CLASS, false, connectionLoader);
        ClassLoader parserLoader = parserUtil.getClassLoader();
        if (parserLoader == null || parserLoader == communityLoader) {
            throw new IllegalStateException(
                    "Community H2 ParserUtil must come from the external driver classloader");
        }

        ClassLoader previous = driverLoader.get();
        driverLoader.set(parserLoader);
        return () -> {
            if (previous == null) {
                driverLoader.remove();
            } else {
                driverLoader.set(previous);
            }
        };
    }

    private InvocationHandler pluginHandler(Object delegate) {
        return new PluginHandler(this, delegate);
    }

    private static final class PluginHandler implements InvocationHandler {
        private final CommunityH2IdentifierCompatibility compatibility;
        private final Object delegate;

        private PluginHandler(CommunityH2IdentifierCompatibility compatibility, Object delegate) {
            this.compatibility = compatibility;
            this.delegate = delegate;
        }

        @Override
        public Object invoke(Object proxy, Method method, Object[] arguments) throws Throwable {
            if (isNoArgMethod(method, "getDbMetaData")) {
                return compatibility.wrapMetadata(
                        CommunityH2IdentifierCompatibility.invoke(delegate, method, arguments));
            }
            if (isNoArgMethod(method, "getSQLIdentifierProcessor")) {
                return compatibility.wrapIdentifierProcessor(
                        CommunityH2IdentifierCompatibility.invoke(delegate, method, arguments));
            }
            return CommunityH2IdentifierCompatibility.invoke(delegate, method, arguments);
        }
    }

    private Object wrapMetadata(Object delegate) {
        if (delegate == null) {
            return null;
        }
        return Proxy.newProxyInstance(
                communityLoader,
                new Class<?>[] {metadataType},
                (proxy, method, arguments) -> isNoArgMethod(method, "getSQLIdentifierProcessor")
                        ? wrapIdentifierProcessor(invoke(delegate, method, arguments))
                        : invoke(delegate, method, arguments));
    }

    private Object wrapIdentifierProcessor(Object delegate) {
        if (delegate == null) {
            return null;
        }
        return Proxy.newProxyInstance(
                communityLoader,
                new Class<?>[] {identifierProcessorType},
                (proxy, method, arguments) -> isConditionalQuote(method)
                        ? quoteIdentifier((String) arguments[0])
                        : invoke(delegate, method, arguments));
    }

    private String quoteIdentifier(String identifier) throws ReflectiveOperationException {
        if (isBlank(identifier)) {
            return identifier;
        }
        ClassLoader currentDriverLoader = driverLoader.get();
        if (currentDriverLoader == null) {
            throw new IllegalStateException(
                    "Community H2 identifier quoting requires an active driver binding");
        }
        Class<?> parserUtil = Class.forName(H2_PARSER_UTIL_CLASS, true, currentDriverLoader);
        Method isSimpleIdentifier =
                parserUtil.getMethod("isSimpleIdentifier", String.class, boolean.class, boolean.class);
        boolean simple = (Boolean) isSimpleIdentifier.invoke(null, identifier, true, false);
        return simple ? identifier : '"' + identifier.replace("\"", "\"\"") + '"';
    }

    private static boolean isConditionalQuote(Method method) {
        if (!method.getName().equals("quoteIdentifier")) {
            return false;
        }
        Class<?>[] parameters = method.getParameterTypes();
        return parameters.length == 1 && parameters[0] == String.class
                || parameters.length == 3 && parameters[0] == String.class;
    }

    private static boolean isNoArgMethod(Method method, String name) {
        return method.getName().equals(name) && method.getParameterCount() == 0;
    }

    private static boolean isBlank(String value) {
        if (value == null || value.isEmpty()) {
            return true;
        }
        for (int index = 0; index < value.length(); index++) {
            if (!Character.isWhitespace(value.charAt(index))) {
                return false;
            }
        }
        return true;
    }

    private static Object invoke(Object delegate, Method method, Object[] arguments)
            throws Throwable {
        try {
            return method.invoke(delegate, arguments);
        } catch (InvocationTargetException failure) {
            throw failure.getCause();
        }
    }

    @FunctionalInterface
    interface DriverBinding extends AutoCloseable {
        DriverBinding NOOP = () -> {};

        @Override
        void close();
    }
}
