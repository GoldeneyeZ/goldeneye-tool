import com.sun.source.tree.AnnotationTree;
import com.sun.source.tree.ClassTree;
import com.sun.source.tree.CompilationUnitTree;
import com.sun.source.tree.ExpressionTree;
import com.sun.source.tree.IdentifierTree;
import com.sun.source.tree.ImportTree;
import com.sun.source.tree.LiteralTree;
import com.sun.source.tree.MemberSelectTree;
import com.sun.source.tree.MethodInvocationTree;
import com.sun.source.tree.MethodTree;
import com.sun.source.tree.NewClassTree;
import com.sun.source.tree.ParenthesizedTree;
import com.sun.source.tree.ThrowTree;
import com.sun.source.tree.Tree;
import com.sun.source.tree.VariableTree;
import com.sun.source.util.JavacTask;
import com.sun.source.util.SourcePositions;
import com.sun.source.util.TreeScanner;
import com.sun.source.util.Trees;

import javax.lang.model.element.Modifier;
import javax.tools.Diagnostic;
import javax.tools.DiagnosticCollector;
import javax.tools.JavaCompiler;
import javax.tools.JavaFileObject;
import javax.tools.StandardJavaFileManager;
import javax.tools.ToolProvider;
import java.io.File;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Deque;
import java.util.LinkedHashSet;
import java.util.LinkedList;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.stream.Collectors;
import java.util.stream.Stream;

/**
 * Source facts for graders/vivwallet-config-change.mjs.
 *
 * usage: java vivwallet-config-change.JavaFacts.java &lt;release&gt; &lt;source root&gt;
 *
 * Parses every .java file under the source root with the JDK compiler at the
 * given language level (parse only: no classpath, nothing is resolved) and
 * prints one JSON document on stdout:
 *
 *   { "errors": ["file:line: message", ...],
 *     "files": [ { "path", "package", "imports": [..], "throws": n,
 *                  "classes":  [ { "name", "annotations", "fields", "ctors", "methods" } ],
 *                  "literals": [ { "value", "ann", "field", "static", "cls" } ],
 *                  "refs": [..], "idents": [..], "selects": [..], "annRefs": [..],
 *                  "news":  [ { "type", "encl", "args" } ],
 *                  "calls": [ { "name", "encl", "text", "args" } ],
 *                  "vars":  [ { "name", "value" } ] } ] }
 *
 * "refs" are the identifiers and selected member names used outside
 * annotations and imports; "idents" only the plain identifiers; "selects" the
 * member selections as "qualifier.member" (qualifier = last name of the
 * selected expression, "?" when it is not a name); "annRefs" the names used
 * inside annotations. A constructor also lists its "paramNames". An
 * argument is { "text", "str" (string literal value or null), "ident" (plain
 * or selected name or null), "param" (index among the enclosing method's
 * parameters, -1 if none) }.
 *
 * Exit code 0 = facts printed (syntax errors, if any, are in "errors"),
 * 2 = usage or no source file.
 */
public class JavaFacts {

    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            System.err.println("usage: JavaFacts <release> <source root>");
            System.exit(2);
        }
        Path root = Paths.get(args[1]).toAbsolutePath().normalize();
        List<File> files;
        try (Stream<Path> paths = Files.walk(root)) {
            files = paths.filter(path -> path.toString().endsWith(".java"))
                         .sorted()
                         .map(Path::toFile)
                         .collect(Collectors.toList());
        }
        if (files.isEmpty()) {
            System.err.println("no .java file under " + root);
            System.exit(2);
        }
        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        DiagnosticCollector<JavaFileObject> diagnostics = new DiagnosticCollector<>();
        StringBuilder out = new StringBuilder();
        try (StandardJavaFileManager fileManager =
                     compiler.getStandardFileManager(diagnostics, Locale.ROOT, StandardCharsets.UTF_8)) {
            JavacTask task = (JavacTask) compiler.getTask(null,
                                                          fileManager,
                                                          diagnostics,
                                                          List.of("-proc:none", "--release", args[0]),
                                                          null,
                                                          fileManager.getJavaFileObjectsFromFiles(files));
            Iterable<? extends CompilationUnitTree> units = task.parse();
            SourcePositions positions = Trees.instance(task).getSourcePositions();

            out.append("{\"errors\":[");
            boolean first = true;
            for (Diagnostic<? extends JavaFileObject> diagnostic : diagnostics.getDiagnostics()) {
                if (diagnostic.getKind() != Diagnostic.Kind.ERROR) {
                    continue;
                }
                String name = diagnostic.getSource() == null
                              ? "<unknown>"
                              : relative(root, Paths.get(diagnostic.getSource().toUri()));
                if (!first) {
                    out.append(',');
                }
                first = false;
                out.append(quote(name + ":" + diagnostic.getLineNumber() + ": " + diagnostic.getMessage(Locale.ROOT)));
            }
            out.append("],\"files\":[");
            first = true;
            for (CompilationUnitTree unit : units) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                String source = unit.getSourceFile().getCharContent(true).toString();
                FileFacts facts = new FileFacts(unit, positions, source);
                facts.scan(unit, null);
                facts.write(out, relative(root, Paths.get(unit.getSourceFile().toUri())));
            }
            out.append("]}");
        }
        PrintStream stdout = new PrintStream(System.out, false, "UTF-8");
        stdout.println(out);
        stdout.flush();
    }

    private static String relative(Path root, Path file) {
        return root.relativize(file.toAbsolutePath().normalize()).toString().replace('\\', '/');
    }

    private static String quote(String text) {
        if (text == null) {
            return "null";
        }
        StringBuilder sb = new StringBuilder("\"");
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '"':
                    sb.append("\\\"");
                    break;
                case '\\':
                    sb.append("\\\\");
                    break;
                case '\n':
                    sb.append("\\n");
                    break;
                case '\r':
                    sb.append("\\r");
                    break;
                case '\t':
                    sb.append("\\t");
                    break;
                default:
                    if (c < 0x20 || c > 0x7e) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
            }
        }
        return sb.append('"').toString();
    }

    private static String list(List<String> jsonItems) {
        return "[" + String.join(",", jsonItems) + "]";
    }

    private static String strings(Iterable<String> values) {
        List<String> items = new ArrayList<>();
        for (String value : values) {
            items.add(quote(value));
        }
        return list(items);
    }

    private static final class ClassFacts {
        String name;
        final List<String> annotations = new ArrayList<>();
        final List<String> fields = new ArrayList<>();
        final List<String> ctors = new ArrayList<>();
        final List<String> methods = new ArrayList<>();

        String json() {
            return "{\"name\":" + quote(name)
                   + ",\"annotations\":" + list(annotations)
                   + ",\"fields\":" + list(fields)
                   + ",\"ctors\":" + list(ctors)
                   + ",\"methods\":" + list(methods) + "}";
        }
    }

    private static final class FileFacts extends TreeScanner<Void, Void> {
        private final CompilationUnitTree unit;
        private final SourcePositions positions;
        private final String source;
        private final List<String> imports = new ArrayList<>();
        private final List<ClassFacts> classes = new ArrayList<>();
        private final List<String> literals = new ArrayList<>();
        private final Set<String> refs = new LinkedHashSet<>();
        private final Set<String> idents = new LinkedHashSet<>();
        private final Set<String> selects = new LinkedHashSet<>();
        private final Set<String> annRefs = new LinkedHashSet<>();
        private final List<String> news = new ArrayList<>();
        private final List<String> calls = new ArrayList<>();
        private final List<String> vars = new ArrayList<>();
        private final Deque<ClassFacts> classStack = new ArrayDeque<>();
        // LinkedList, not ArrayDeque: the NO_METHOD marker is null
        private final Deque<MethodTree> methodStack = new LinkedList<>();
        private int annotationDepth = 0;
        private int throwCount = 0;
        // the field whose direct initializer is being visited (null otherwise)
        private VariableTree initializedField = null;
        private Tree initializerLiteral = null;

        FileFacts(CompilationUnitTree unit, SourcePositions positions, String source) {
            this.unit = unit;
            this.positions = positions;
            this.source = source;
        }

        void write(StringBuilder out, String path) {
            out.append("{\"path\":").append(quote(path))
               .append(",\"package\":").append(quote(unit.getPackageName() == null ? "" : unit.getPackageName().toString()))
               .append(",\"imports\":").append(strings(imports))
               .append(",\"throws\":").append(throwCount)
               .append(",\"classes\":").append(list(classes.stream().map(ClassFacts::json).collect(Collectors.toList())))
               .append(",\"literals\":").append(list(literals))
               .append(",\"refs\":").append(strings(refs))
               .append(",\"idents\":").append(strings(idents))
               .append(",\"selects\":").append(strings(selects))
               .append(",\"annRefs\":").append(strings(annRefs))
               .append(",\"news\":").append(list(news))
               .append(",\"calls\":").append(list(calls))
               .append(",\"vars\":").append(list(vars))
               .append('}');
        }

        private String text(Tree tree) {
            if (tree == null) {
                return null;
            }
            long start = positions.getStartPosition(unit, tree);
            long end = positions.getEndPosition(unit, tree);
            if (start < 0 || end < 0 || end > source.length() || start > end) {
                return tree.toString();
            }
            return source.substring((int) start, (int) end);
        }

        private static String simpleName(Tree tree) {
            if (tree instanceof IdentifierTree) {
                return ((IdentifierTree) tree).getName().toString();
            }
            if (tree instanceof MemberSelectTree) {
                return ((MemberSelectTree) tree).getIdentifier().toString();
            }
            String text = tree.toString();
            int generic = text.indexOf('<');
            if (generic >= 0) {
                text = text.substring(0, generic);
            }
            return text.substring(text.lastIndexOf('.') + 1);
        }

        private static Tree unwrap(Tree tree) {
            while (tree instanceof ParenthesizedTree) {
                tree = ((ParenthesizedTree) tree).getExpression();
            }
            return tree;
        }

        private static String stringValue(Tree tree) {
            tree = unwrap(tree);
            if (tree instanceof LiteralTree && ((LiteralTree) tree).getValue() instanceof String) {
                return (String) ((LiteralTree) tree).getValue();
            }
            return null;
        }

        private String annotationJson(AnnotationTree annotation) {
            return "{\"name\":" + quote(simpleName(annotation.getAnnotationType()))
                   + ",\"text\":" + quote(text(annotation)) + "}";
        }

        private String annotationsJson(List<? extends AnnotationTree> annotations) {
            return list(annotations.stream().map(this::annotationJson).collect(Collectors.toList()));
        }

        private String argsJson(List<? extends ExpressionTree> arguments) {
            List<String> items = new ArrayList<>();
            MethodTree method = methodStack.peek();
            for (ExpressionTree argument : arguments) {
                Tree plain = unwrap(argument);
                String ident = null;
                int param = -1;
                if (plain instanceof IdentifierTree) {
                    ident = ((IdentifierTree) plain).getName().toString();
                    if (method != null) {
                        List<? extends VariableTree> parameters = method.getParameters();
                        for (int i = 0; i < parameters.size(); i++) {
                            if (parameters.get(i).getName().contentEquals(ident)) {
                                param = i;
                            }
                        }
                    }
                } else if (plain instanceof MemberSelectTree) {
                    ident = ((MemberSelectTree) plain).getIdentifier().toString();
                }
                items.add("{\"text\":" + quote(clip(text(argument)))
                          + ",\"str\":" + quote(stringValue(argument))
                          + ",\"ident\":" + quote(ident)
                          + ",\"param\":" + param + "}");
            }
            return list(items);
        }

        private static String clip(String text) {
            if (text == null) {
                return null;
            }
            return text.length() > 800 ? text.substring(0, 800) : text;
        }

        private String enclosing() {
            MethodTree method = methodStack.peek();
            return method == null ? null : method.getName().toString();
        }

        @Override
        public Void visitImport(ImportTree node, Void unused) {
            imports.add((node.isStatic() ? "static " : "") + node.getQualifiedIdentifier().toString());
            return null;
        }

        @Override
        public Void visitClass(ClassTree node, Void unused) {
            ClassFacts facts = new ClassFacts();
            facts.name = node.getSimpleName().toString();
            for (AnnotationTree annotation : node.getModifiers().getAnnotations()) {
                facts.annotations.add(annotationJson(annotation));
            }
            for (Tree member : node.getMembers()) {
                if (member instanceof VariableTree) {
                    VariableTree field = (VariableTree) member;
                    Set<Modifier> flags = field.getModifiers().getFlags();
                    facts.fields.add("{\"name\":" + quote(field.getName().toString())
                                     + ",\"type\":" + quote(field.getType() == null ? null : field.getType().toString())
                                     + ",\"static\":" + flags.contains(Modifier.STATIC)
                                     + ",\"final\":" + flags.contains(Modifier.FINAL)
                                     + ",\"annotations\":" + annotationsJson(field.getModifiers().getAnnotations())
                                     + ",\"init\":" + quote(clip(text(field.getInitializer())))
                                     + ",\"initString\":" + quote(field.getInitializer() == null ? null : stringValue(field.getInitializer()))
                                     + "}");
                } else if (member instanceof MethodTree) {
                    MethodTree method = (MethodTree) member;
                    List<String> parameterAnnotations = new ArrayList<>();
                    for (VariableTree parameter : method.getParameters()) {
                        for (AnnotationTree annotation : parameter.getModifiers().getAnnotations()) {
                            parameterAnnotations.add(annotationJson(annotation));
                        }
                    }
                    String json = "{\"name\":" + quote(method.getName().toString())
                                  + ",\"params\":" + method.getParameters().size()
                                  + ",\"paramNames\":" + strings(method.getParameters().stream()
                                                                       .map(parameter -> parameter.getName().toString())
                                                                       .collect(Collectors.toList()))
                                  + ",\"returnType\":" + quote(method.getReturnType() == null ? null : method.getReturnType().toString())
                                  + ",\"static\":" + method.getModifiers().getFlags().contains(Modifier.STATIC)
                                  + ",\"annotations\":" + annotationsJson(method.getModifiers().getAnnotations())
                                  + ",\"paramAnnotations\":" + list(parameterAnnotations) + "}";
                    if (method.getName().contentEquals("<init>")) {
                        facts.ctors.add(json);
                    } else {
                        facts.methods.add(json);
                    }
                }
            }
            classes.add(facts);
            classStack.push(facts);
            // a class body starts a new "enclosing method" scope
            methodStack.push(NO_METHOD);
            try {
                return super.visitClass(node, unused);
            } finally {
                methodStack.pop();
                classStack.pop();
            }
        }

        @Override
        public Void visitMethod(MethodTree node, Void unused) {
            methodStack.push(node);
            try {
                return super.visitMethod(node, unused);
            } finally {
                methodStack.pop();
            }
        }

        @Override
        public Void visitAnnotation(AnnotationTree node, Void unused) {
            annotationDepth++;
            try {
                return super.visitAnnotation(node, unused);
            } finally {
                annotationDepth--;
            }
        }

        @Override
        public Void visitVariable(VariableTree node, Void unused) {
            String value = node.getInitializer() == null ? null : stringValue(node.getInitializer());
            if (value != null) {
                vars.add("{\"name\":" + quote(node.getName().toString()) + ",\"value\":" + quote(value) + "}");
            }
            boolean isField = methodStack.peek() == NO_METHOD;
            VariableTree previousField = initializedField;
            Tree previousLiteral = initializerLiteral;
            if (isField && value != null) {
                initializedField = node;
                initializerLiteral = unwrap(node.getInitializer());
            }
            try {
                return super.visitVariable(node, unused);
            } finally {
                initializedField = previousField;
                initializerLiteral = previousLiteral;
            }
        }

        @Override
        public Void visitLiteral(LiteralTree node, Void unused) {
            if (node.getValue() instanceof String) {
                boolean direct = initializedField != null && node == initializerLiteral;
                ClassFacts owner = classStack.peek();
                literals.add("{\"value\":" + quote((String) node.getValue())
                             + ",\"ann\":" + (annotationDepth > 0)
                             + ",\"field\":" + quote(direct ? initializedField.getName().toString() : null)
                             + ",\"static\":" + (direct && initializedField.getModifiers().getFlags().contains(Modifier.STATIC))
                             + ",\"cls\":" + quote(owner == null ? null : owner.name)
                             + "}");
            }
            return super.visitLiteral(node, unused);
        }

        @Override
        public Void visitIdentifier(IdentifierTree node, Void unused) {
            (annotationDepth > 0 ? annRefs : refs).add(node.getName().toString());
            if (annotationDepth == 0) {
                idents.add(node.getName().toString());
            }
            return super.visitIdentifier(node, unused);
        }

        @Override
        public Void visitMemberSelect(MemberSelectTree node, Void unused) {
            (annotationDepth > 0 ? annRefs : refs).add(node.getIdentifier().toString());
            if (annotationDepth == 0) {
                Tree qualifier = unwrap(node.getExpression());
                String name = qualifier instanceof IdentifierTree || qualifier instanceof MemberSelectTree
                              ? simpleName(qualifier)
                              : "?";
                selects.add(name + "." + node.getIdentifier());
            }
            return super.visitMemberSelect(node, unused);
        }

        @Override
        public Void visitThrow(ThrowTree node, Void unused) {
            throwCount++;
            return super.visitThrow(node, unused);
        }

        @Override
        public Void visitNewClass(NewClassTree node, Void unused) {
            news.add("{\"type\":" + quote(simpleName(node.getIdentifier()))
                     + ",\"encl\":" + quote(enclosing())
                     + ",\"args\":" + argsJson(node.getArguments()) + "}");
            return super.visitNewClass(node, unused);
        }

        @Override
        public Void visitMethodInvocation(MethodInvocationTree node, Void unused) {
            calls.add("{\"name\":" + quote(simpleName(node.getMethodSelect()))
                      + ",\"encl\":" + quote(enclosing())
                      + ",\"text\":" + quote(clip(text(node)))
                      + ",\"args\":" + argsJson(node.getArguments()) + "}");
            return super.visitMethodInvocation(node, unused);
        }
    }

    // marker pushed while visiting a class body, so that field initializers and
    // initializer blocks are not attributed to an outer method
    private static final MethodTree NO_METHOD = null;
}
