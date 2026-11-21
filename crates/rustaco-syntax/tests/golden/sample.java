package demo;

public class Greeter {
    private final String name;

    public Greeter(String name) { this.name = name; }

    @Override
    public String toString() { return "Hi " + name; }
}
