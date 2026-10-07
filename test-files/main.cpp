#include <iostream>

int arr[1000000];

int main() {
    int a, b;
    std::cin >> a >> b;
    std::cout << (a + b) << '\n';
    arr[0] = a;
    arr[1] = b;
    arr[2] = a + b;
    return 0;
}
