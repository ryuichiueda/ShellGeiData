#include <iostream>
#include <string>
using namespace std;
void aho(void);
string nazo(void);

void aho(void)
{
	cout << nazo() << endl;
}

string nazo(void)
{
	return "謎";
}

int main(int argc, char const* argv[])
{
	aho();
	return 0;
}
