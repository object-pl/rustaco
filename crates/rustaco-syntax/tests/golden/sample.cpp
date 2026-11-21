#include <vector>

template <typename T>
class Stack {
public:
    void push(const T& v) { items.push_back(v); }
private:
    std::vector<T> items;
};
